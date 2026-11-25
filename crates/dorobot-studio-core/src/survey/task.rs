//! Content-addressed task versions carry the scene, its pinned assets and explicit instance bindings.
use super::{
    asset::Asset,
    scene::{validation::Support, Revision, Scene},
    Survey,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{ErrorKind, Write},
    path::PathBuf,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoundTask {
    pub schema: String,
    pub robot: String,
    pub instruction: String,
    pub scene: Scene,
    pub assets: Vec<Asset>,
    pub pickup: String,
    pub placement: Support,
    pub observations: Vec<String>,
    pub success_criteria: Vec<String>,
}

impl Survey {
    pub fn pinned_assets(&self, scene: &Scene) -> Result<HashMap<Revision, Asset>> {
        let mut assets = HashMap::new();
        for obj in &scene.instances {
            if !assets.contains_key(&obj.asset) {
                assets.insert(obj.asset.clone(), self.asset_revision(&obj.asset)?);
            }
        }
        Ok(assets)
    }

    pub fn bind_pickup(
        &self,
        scene: &Revision,
        instruction: &str,
        pickup: &str,
        placement: &Support,
    ) -> Result<BoundTask> {
        ensure!(!instruction.trim().is_empty(), "Enter a task instruction");
        let scene = self.scene_revision(scene)?;
        let assets = self.pinned_assets(&scene)?;
        let source = scene
            .instances
            .iter()
            .find(|i| i.id == pickup)
            .context("Bound pickup object is missing from this scene revision")?;
        let dest = scene
            .instances
            .iter()
            .find(|i| i.id == placement.instance)
            .context("Bound placement object is missing from this scene revision")?;
        ensure!(
            source.id != dest.id,
            "Select separate pickup and support objects"
        );
        let object = &assets[&source.asset].physics;
        ensure!(
            object.graspable.value && object.movable.value,
            "Select a movable, graspable object"
        );
        let cue = &object.appearance.value;
        ensure!(
            matches!(cue.as_str(), "red" | "green" | "blue"),
            "The Nano pickup controller needs a red, green or blue object"
        );
        ensure!(
            scene
                .instances
                .iter()
                .filter(|i| assets[&i.asset].physics.appearance.value == *cue)
                .count()
                == 1,
            "The onboard RGB controller needs a distinguishable target appearance"
        );
        let support = &assets[&dest.asset];
        ensure!(
            !support.physics.movable.value
                && support
                    .geometry
                    .surfaces
                    .iter()
                    .any(|s| s.name == placement.surface),
            "Bound placement surface is missing from this scene revision"
        );
        ensure!(
            scene
                .instances
                .iter()
                .filter(|i| {
                    let a = &assets[&i.asset];
                    !a.geometry.surfaces.is_empty()
                        && a.physics.appearance.value == support.physics.appearance.value
                })
                .count()
                == 1,
            "The RGB controller needs a distinguishable placement surface appearance"
        );
        if let Some(issue) = scene.issues(&assets).first() {
            let who = scene
                .instances
                .iter()
                .find(|i| i.id == issue.instance)
                .map(|i| i.name.as_str())
                .unwrap_or("Scene object");
            anyhow::bail!("{who}: {}", issue.message);
        }
        let mut assets = assets.into_values().collect::<Vec<_>>();
        assets.sort_by(|a, b| (&a.id, a.revision).cmp(&(&b.id, b.revision)));
        Ok(BoundTask{
            schema:"aisle-studio-nano-pickup-v1".into(),robot:"Nano".into(),instruction:instruction.trim().into(),scene,assets,
            pickup:pickup.into(),placement:placement.clone(),
            observations:["head RGB","left wrist RGB","right wrist RGB","joint and wheel encoders"].map(String::from).to_vec(),
            success_criteria:["Opposing fingers carry the selected object clear of support for at least 20 physics steps",
                "Release on the selected surface with 100 consecutive loaded contact steps and less than 3 mm settling motion",
                "Wheel support throughout with no prohibited collisions, joint excursions over 0.005 radians or rejected motor writes"].map(String::from).to_vec(),
        })
    }

    pub fn activate_task(&self, task: &BoundTask) -> Result<String> {
        let raw = serde_json::to_vec_pretty(task)?;
        let sha = format!("{:x}", Sha256::digest(&raw));
        let root = self.root.join("tasks");
        fs::create_dir_all(&root)?;
        let path = root.join(format!("{sha}.json"));
        let mut dst = tempfile::NamedTempFile::new_in(&root)?;
        dst.write_all(&raw)?;
        dst.as_file().sync_all()?;
        if let Err(e) = dst.persist_noclobber(&path) {
            if e.error.kind() != ErrorKind::AlreadyExists {
                return Err(e.error.into());
            }
            ensure!(fs::read(&path)? == raw, "Saved task contents changed");
        }
        let mut active = tempfile::NamedTempFile::new_in(&self.root)?;
        active.write_all(sha.as_bytes())?;
        active.as_file().sync_all()?;
        active
            .persist(self.root.join("active-task"))
            .map_err(|e| e.error)?;
        Ok(sha)
    }

    pub fn active_task(&self) -> Result<Option<(String, PathBuf)>> {
        let sha = match fs::read_to_string(self.root.join("active-task")) {
            Ok(v) => v,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        ensure!(
            sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid active task identifier"
        );
        let path = self.root.join("tasks").join(format!("{sha}.json"));
        ensure!(
            format!("{:x}", Sha256::digest(fs::read(&path)?)) == sha,
            "Active task contents changed"
        );
        Ok(Some((sha, path)))
    }

    pub fn clear_active_task(&self) -> Result<()> {
        match fs::remove_file(self.root.join("active-task")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
