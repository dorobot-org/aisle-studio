//! Provider-backed scene edits and operator review.
use super::*;
use dorobot_studio_core::survey::scene::agent_edit::{Kind, Proposal};
use serde_json::json;
use std::{io::Write, process::Stdio, sync::mpsc};

#[derive(Default)]
pub(super) struct Review {
    waiting: Option<mpsc::Receiver<Result<Proposal, String>>>,
    plan: Option<Proposal>,
    choices: Vec<Option<String>>,
    pub(super) source: Option<EditSource>,
}

fn provider_name(provider: &str) -> &str {
    match provider {
        "codex" => "Codex",
        "octos-deepseek" => "DeepSeek via Octos",
        "local" => "Local commands",
        value => value,
    }
}

impl SceneEditorPane {
    pub(super) fn review_active(&self) -> bool {
        self.candidate.is_some() || self.review.waiting.is_some() || self.review.plan.is_some()
    }

    pub(super) fn request_edit(
        &mut self,
        cx: &mut Cx,
        provider: &str,
        instruction: &str,
    ) -> Result<(), String> {
        if instruction.is_empty() || instruction.chars().count() > 4000 {
            return Err("Enter a scene instruction of up to 4,000 characters".into());
        }
        let scene = &self.editor.scene;
        let mut instances = Vec::new();
        for i in &scene.instances {
            let asset = self
                .pinned
                .get(&i.asset)
                .ok_or("Refresh the missing asset revision")?;
            instances.push(json!({
                "id": i.id, "name": i.name, "asset_name": asset.name, "asset": i.asset,
                "position_m": i.position_m, "yaw_deg": i.yaw_deg, "support": i.support,
            }));
        }
        let assets: Vec<_> = self
            .catalog
            .iter()
            .map(|asset| {
                let dimensions = asset
                    .dimensions
                    .each_ref()
                    .map(|d| d.as_ref().map(|v| v.metres()));
                json!({
                    "id": asset.id, "name": asset.name, "revision": asset.revision,
                    "dimensions_m": dimensions, "surfaces": asset.geometry.surfaces,
                })
            })
            .collect();
        let input = serde_json::to_vec(&json!({
            "instruction": instruction, "room_m": scene.room_m,
            "instances": instances, "assets": assets,
        }))
        .map_err(|e| e.to_string())?;
        let root = runtime::root();
        let mut cmd =
            dorobot_studio_core::api::aisle::python_command(&root).map_err(|e| e.to_string())?;
        cmd.arg(root.join("tools/aisle_workbench/scene_composition.py"))
            .arg(provider)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let mut child = cmd.spawn().map_err(|e| e.to_string())?;
                let wrote = child
                    .stdin
                    .take()
                    .ok_or("Missing scene input pipe")?
                    .write_all(&input);
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                if !out.status.success() {
                    return Err(String::from_utf8_lossy(&out.stderr)
                        .lines()
                        .last()
                        .unwrap_or("Scene proposal failed")
                        .to_owned());
                }
                wrote.map_err(|e| e.to_string())?;
                serde_json::from_slice::<Proposal>(&out.stdout).map_err(|e| e.to_string())
            })();
            let _ = tx.send(result);
            SignalToUI::set_ui_signal();
        });
        self.review.waiting = Some(rx);
        self.sync(cx);
        self.note(
            cx,
            &format!(
                "Waiting for {} to propose room edits…",
                provider_name(provider)
            ),
        );
        Ok(())
    }

    pub(super) fn poll_edit(&mut self, cx: &mut Cx) {
        let Some(rx) = &self.review.waiting else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(v) => v,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("The scene proposal worker stopped".to_owned()),
        };
        self.review.waiting = None;
        let result = result.and_then(|p| {
            self.review.choices = p
                .initial_choices(&self.editor.scene, &self.catalog)
                .map_err(|e| e.to_string())?;
            self.review.plan = Some(p);
            self.next_reference(cx)
        });
        if let Err(e) = result {
            self.review = Default::default();
            self.sync(cx);
            self.note(cx, &e);
        }
    }

    fn next_reference(&mut self, cx: &mut Cx) -> Result<(), String> {
        let plan = self.review.plan.as_ref().ok_or("Missing scene proposal")?;
        if let Some(index) = self.review.choices.iter().position(Option::is_none) {
            let r = &plan.references[index];
            let labels = r
                .candidates
                .iter()
                .map(|id| {
                    match r.kind {
                        Kind::Instance => self
                            .editor
                            .scene
                            .instances
                            .iter()
                            .find(|o| &o.id == id)
                            .map(|o| {
                                format!(
                                    "{} ({:.2}, {:.2} m)",
                                    o.name, o.position_m[0], o.position_m[1]
                                )
                            }),
                        Kind::Asset => self
                            .catalog
                            .iter()
                            .find(|a| &a.id == id)
                            .map(|a| format!("{} · v{}", a.name, a.revision)),
                    }
                    .unwrap_or_else(|| id.clone())
                })
                .collect();
            let dd = self.view.drop_down(cx, ids!(reference_choices));
            dd.set_labels(cx, labels);
            dd.set_selected_item(cx, 0);
            self.view
                .label(cx, ids!(reference_prompt))
                .set_text(cx, &format!("Which object matches “{}”?", r.phrase));
            self.note(cx, "Choose the intended object to preview the edit");
        } else {
            let (scene, source) = plan
                .materialize(
                    &self.editor.scene,
                    &self.review.choices,
                    &self.catalog,
                    &self.pinned,
                )
                .map_err(|e| e.to_string())?;
            self.selected = source
                .affected
                .iter()
                .find(|id| scene.instances.iter().any(|i| &i.id == *id))
                .cloned();
            self.candidate = Some(scene);
            self.review.source = Some(source);
            self.note(cx, "Review the proposed pose in the inspector");
        }
        self.sync(cx);
        Ok(())
    }

    pub(super) fn confirm_reference(&mut self, cx: &mut Cx) -> Result<(), String> {
        let idx = self
            .review
            .choices
            .iter()
            .position(Option::is_none)
            .ok_or("No object choice is pending")?;
        let plan = self.review.plan.as_ref().ok_or("Missing scene proposal")?;
        let chosen = self
            .view
            .drop_down(cx, ids!(reference_choices))
            .selected_item();
        let id = plan.references[idx]
            .candidates
            .get(chosen)
            .ok_or("Choose a listed object")?
            .clone();
        self.review.choices[idx] = Some(id);
        if let Err(e) = self.next_reference(cx) {
            self.review = Default::default();
            self.sync(cx);
            return Err(e);
        }
        Ok(())
    }

    pub(super) fn review_labels(&self, cx: &mut Cx) {
        let resolving =
            self.review.plan.is_some() && self.review.choices.iter().any(Option::is_none);
        self.view
            .widget(cx, ids!(reference_row))
            .set_visible(cx, resolving);
        let source = self
            .review
            .source
            .as_ref()
            .or_else(|| self.editor.scene.edit_history.last());
        let label = if let Some(plan) = &self.review.plan {
            format!(
                "{} · {} · {}",
                provider_name(&plan.provider),
                plan.model,
                plan.reason
            )
        } else if let Some(s) = source {
            let model = s
                .model
                .as_deref()
                .map(|m| format!(" · {m}"))
                .unwrap_or_default();
            format!(
                "{}{} · {}",
                provider_name(&s.provider),
                model,
                s.instruction
            )
        } else {
            String::new()
        };
        self.view.label(cx, ids!(source_label)).set_text(cx, &label);
        self.view
            .widget(cx, ids!(source_label))
            .set_visible(cx, !label.is_empty());
    }
}
