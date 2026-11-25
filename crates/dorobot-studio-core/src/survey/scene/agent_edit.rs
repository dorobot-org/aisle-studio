//! Resolve model references in the editor and build an undoable layout from typed operations.
use super::{validation::Support, Revision, Scene};
use crate::survey::asset::Asset;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditSource {
    pub provider: String,
    pub model: Option<String>,
    pub instruction: String,
    pub request_sha256: Option<String>,
    pub reply_sha256: Option<String>,
    pub bindings: Vec<(String, String)>,
    pub proposed_operations: Vec<Operation>,
    pub affected: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operator_adjusted: Vec<String>,
}

#[derive(Deserialize)]
pub struct Proposal {
    pub provider: String,
    pub model: String,
    pub instruction: String,
    pub reason: String,
    pub request_sha256: String,
    pub reply_sha256: String,
    pub references: Vec<Reference>,
    operations: Vec<Operation>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Asset,
    Instance,
}

#[derive(Deserialize)]
pub struct Reference {
    pub phrase: String,
    pub kind: Kind,
    pub candidates: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Action {
    Add,
    Translate,
    Rotate,
    Pose,
    Relative,
    Snap,
    Remove,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Operation {
    action: Action,
    subject: usize,
    anchor: i32,
    xyz_m: [f64; 3],
    yaw_deg: f64,
    surface: String,
}

impl Proposal {
    pub fn initial_choices(&self, scene: &Scene, catalog: &[Asset]) -> Result<Vec<Option<String>>> {
        ensure!(
            !self.references.is_empty() && !self.operations.is_empty(),
            "The proposal has no scene edits"
        );
        self.references
            .iter()
            .map(|r| {
                ensure!(!r.candidates.is_empty(), "No match for {}", r.phrase);
                for id in &r.candidates {
                    let exists = match r.kind {
                        Kind::Instance => scene.instances.iter().any(|i| &i.id == id),
                        Kind::Asset => catalog.iter().any(|a| &a.id == id),
                    };
                    ensure!(exists, "{} refers to a missing object", r.phrase);
                }
                Ok((r.candidates.len() == 1).then(|| r.candidates[0].clone()))
            })
            .collect()
    }

    pub fn materialize(
        &self,
        original: &Scene,
        choices: &[Option<String>],
        catalog: &[Asset],
        pinned: &HashMap<Revision, Asset>,
    ) -> Result<(Scene, EditSource)> {
        ensure!(
            choices.len() == self.references.len(),
            "Incomplete object selections"
        );
        let mut bindings = Vec::new();
        for (r, choice) in self.references.iter().zip(choices) {
            let id = choice
                .as_ref()
                .filter(|id| r.candidates.contains(id))
                .context("Choose a listed object")?;
            bindings.push((r.phrase.clone(), id.clone()));
        }
        let mut scene = original.clone();
        for op in &self.operations {
            let reference = self
                .references
                .get(op.subject)
                .context("Missing subject reference")?;
            let id = &bindings[op.subject].1;
            ensure!(
                op.xyz_m.iter().all(|n| n.is_finite()) && op.yaw_deg.is_finite(),
                "Invalid proposed pose"
            );
            ensure!(
                (op.action == Action::Add) == (reference.kind == Kind::Asset),
                "Action has the wrong reference type"
            );
            if op.action == Action::Add {
                let asset = catalog
                    .iter()
                    .find(|a| &a.id == id)
                    .context("Missing proposed asset")?;
                scene.place(asset, op.xyz_m)?;
                scene.instances.last_mut().unwrap().yaw_deg = op.yaw_deg;
                continue;
            }
            let index = scene
                .instances
                .iter()
                .position(|i| &i.id == id)
                .context("The proposed subject is absent")?;
            match op.action {
                Action::Translate => {
                    let item = &mut scene.instances[index];
                    for (v, delta) in item.position_m.iter_mut().zip(op.xyz_m) {
                        *v += delta;
                    }
                }
                Action::Rotate => scene.instances[index].yaw_deg += op.yaw_deg,
                Action::Pose => {
                    scene.instances[index].position_m = op.xyz_m;
                    scene.instances[index].yaw_deg = op.yaw_deg;
                }
                Action::Remove => {
                    scene.instances.remove(index);
                    scene.rules.retain(|r| r.instance.as_ref() != Some(id));
                }
                Action::Relative | Action::Snap => {
                    let anchor_idx = usize::try_from(op.anchor).context("Missing anchor")?;
                    let anchor = &bindings.get(anchor_idx).context("Unknown anchor")?.1;
                    ensure!(
                        id != anchor && self.references[anchor_idx].kind == Kind::Instance,
                        "Choose a different existing anchor"
                    );
                    let target = scene
                        .instances
                        .iter()
                        .find(|i| &i.id == anchor)
                        .context("Anchor was removed")?;
                    if op.action == Action::Snap {
                        scene.snap_to(
                            id,
                            Some(Support {
                                instance: anchor.clone(),
                                surface: op.surface.clone(),
                            }),
                            pinned,
                        )?;
                    } else {
                        let location = target.position_m;
                        scene.instances[index].position_m =
                            std::array::from_fn(|n| location[n] + op.xyz_m[n]);
                    }
                }
                Action::Add => unreachable!(),
            }
        }
        scene.check()?;
        let affected = changed_ids(original, &scene);
        ensure!(
            !affected.is_empty(),
            "The proposed operations leave the layout unchanged"
        );
        Ok((
            scene,
            EditSource {
                instruction: self.instruction.clone(),
                provider: self.provider.clone(),
                model: Some(self.model.clone()),
                request_sha256: Some(self.request_sha256.clone()),
                reply_sha256: Some(self.reply_sha256.clone()),
                bindings,
                affected,
                proposed_operations: self.operations.clone(),
                operator_adjusted: vec![],
            },
        ))
    }
}

impl EditSource {
    pub fn local(instruction: String, before: &Scene, after: &Scene) -> Self {
        Self {
            provider: "local".into(),
            model: None,
            instruction,
            request_sha256: None,
            reply_sha256: None,
            bindings: vec![],
            proposed_operations: vec![],
            affected: changed_ids(before, after),
            operator_adjusted: vec![],
        }
    }
}

pub fn changed_ids(before: &Scene, after: &Scene) -> Vec<String> {
    let mut ids = vec![];
    for item in &after.instances {
        if before.instances.iter().find(|old| old.id == item.id) != Some(item) {
            ids.push(item.id.clone());
        }
    }
    ids.extend(
        before
            .instances
            .iter()
            .filter(|i| !after.instances.iter().any(|v| v.id == i.id))
            .map(|i| i.id.clone()),
    );
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn two_adds_pin_the_same_asset_without_reusing_an_instance_identity() {
        let chair = Asset {
            id: "chair".into(),
            name: "Chair".into(),
            revision: 2,
            ..Asset::default()
        };
        let catalog = [chair];
        let original = Scene::default();
        let mut plan: Proposal = serde_json::from_value(json!({
            "provider":"codex", "model":"test", "instruction":"Place two chairs",
            "reason":"two placements", "request_sha256":"test", "reply_sha256":"test",
            "references":[{"phrase":"chairs","candidates":["chair"],"kind":"asset"}],
            "operations":[
                {"action":"add","subject":0,"anchor":-1,"xyz_m":[-1,0,0],"yaw_deg":15,"surface":""},
                {"action":"add","subject":0,"anchor":-1,"xyz_m":[1,0,0],"yaw_deg":30,"surface":""}
            ]
        }))
        .unwrap();
        let choices = plan.initial_choices(&original, &catalog).unwrap();
        let (room, source) = plan
            .materialize(&original, &choices, &catalog, &HashMap::new())
            .unwrap();
        assert_eq!(room.instances.len(), 2);
        assert_eq!(source.affected.len(), 2);
        assert_ne!(room.instances[0].id, room.instances[1].id);
        assert_eq!(room.instances[0].asset, room.instances[1].asset);
        assert_eq!(room.instances[0].asset.revision, 2);
        assert_eq!(room.instances[1].yaw_deg, 30.);
        plan.operations[1].action = Action::Relative;
        assert!(plan
            .materialize(&original, &choices, &catalog, &HashMap::new())
            .is_err());
        assert!(original.instances.is_empty());
    }
}
