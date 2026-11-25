//! Select installed Octos app skills by source and content digest on the Task page.
use super::*;
use dorobot_studio_core::runtime;
use serde_json::Value;
use std::{io::Write, process::Stdio, sync::mpsc};

#[derive(Default)]
pub(super) struct Skills {
    open: bool,
    robot: Option<Robot>,
    catalog: Value,
    index: Option<usize>,
    pending: Option<mpsc::Receiver<Result<Value, String>>>,
    error: String,
}

fn brief(pin: &Value) -> String {
    format!(
        "{} {} · {} · {}",
        pin["name"].as_str().unwrap_or(""),
        pin["version"].as_str().unwrap_or(""),
        pin["source"].as_str().unwrap_or(""),
        pin["package_sha256"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(12)
            .collect::<String>()
    )
}

impl Panel {
    fn catalog_operation(&mut self, robot: Robot, operation: &'static str, pin: Option<Value>) {
        let root = runtime::root();
        let state = runtime::user_data().join("aisle/spec-skills");
        let (tx, rx) = mpsc::channel();
        self.skills.pending = Some(rx);
        self.skills.error.clear();
        std::thread::spawn(move || {
            let result = (|| {
                let mut cmd = dorobot_studio_core::api::aisle::python_command(&root)
                    .map_err(|e| e.to_string())?;
                let mut child = cmd
                    .arg(root.join("tools/aisle_workbench/spec_catalog.py"))
                    .arg(operation)
                    .args(["--robot", robot.name(), "--state"])
                    .arg(state)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                if let Some(pin) = pin {
                    child
                        .stdin
                        .take()
                        .ok_or("Missing skill selection pipe")?
                        .write_all(pin.to_string().as_bytes())
                        .map_err(|e| e.to_string())?;
                }
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                if !out.status.success() {
                    return Err(String::from_utf8_lossy(&out.stderr)
                        .lines()
                        .last()
                        .unwrap_or("Skill library unavailable")
                        .to_owned());
                }
                serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
            })();
            let _ = tx.send(result);
            SignalToUI::set_ui_signal();
        });
    }

    pub(super) fn skill_sync(&mut self, cx: &mut Cx, w: &Workbench) {
        self.root
            .widget(cx, ids!(skill_panel))
            .set_visible(cx, self.skills.open);
        let active = &w.plan["spec_skill"]["package"];
        self.root
            .widget(cx, ids!(skill_applied))
            .set_visible(cx, active.is_object());
        if active.is_object() {
            self.label(
                cx,
                ids!(skill_applied),
                &format!("Prepared skill · {}", brief(active)),
            );
        }
        if !self.skills.open {
            return;
        }
        if self.skills.robot != Some(w.robot) {
            self.skills.robot = Some(w.robot);
            self.skills.index = None;
            self.skills.catalog = Value::Null;
            self.catalog_operation(w.robot, "list", None);
        }
        let reply = self
            .skills
            .pending
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err("Skill library worker stopped".into())),
            });
        if let Some(reply) = reply {
            self.skills.pending = None;
            match reply {
                Ok(v) => {
                    let entries = v["entries"].as_array().cloned().unwrap_or_default();
                    self.skills.index = entries.iter().position(|p| p == &v["selected"]);
                    let labels = std::iter::once("Choose installed source".to_owned())
                        .chain(entries.iter().map(brief))
                        .collect();
                    let dd = self.root.drop_down(cx, ids!(skill_sources));
                    dd.set_labels(cx, labels);
                    dd.set_selected_item(cx, self.skills.index.map_or(0, |i| i + 1));
                    self.skills.catalog = v;
                }
                Err(e) => self.skills.error = e,
            }
        }
        let pending = self.skills.pending.is_some();
        let pin = self
            .skills
            .index
            .and_then(|i| self.skills.catalog["entries"].get(i));
        let selected = &self.skills.catalog["selected"];
        let info = pin
            .map(|p| {
                let words = |key: &str| {
                    p[key]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                format!(
                    "{} · {} · sensors {} · actions {}\nSHA-256 {}",
                    brief(p),
                    w.robot.name(),
                    words("sensors"),
                    words("action_units"),
                    p["package_sha256"].as_str().unwrap_or("")
                )
            })
            .unwrap_or_default();
        self.label(cx, ids!(skill_contract), &info);
        let errors = self.skills.catalog["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        let mut status = if pending {
            "Reading installed skill…".into()
        } else if !self.skills.error.is_empty() {
            self.skills.error.clone()
        } else if selected.is_object() {
            format!("Next preparation · {}", brief(selected))
        } else {
            "Next preparation · direct AISLE".into()
        };
        if !errors.is_empty() {
            status.push('\n');
            status.push_str(&errors);
        }
        self.label(cx, ids!(skill_status), &status);
        for (id, enabled) in [
            (ids!(skill_use) as &[LiveId], pin.is_some()),
            (ids!(skill_clear), selected.is_object()),
            (ids!(skill_refresh), true),
        ] {
            self.root
                .button(cx, id)
                .set_enabled(cx, enabled && !pending && !w.busy());
        }
    }

    pub(super) fn skill_actions(&mut self, cx: &mut Cx, actions: &Actions, w: &Workbench) -> bool {
        if self.root.button(cx, ids!(skill_mode)).clicked(actions) {
            self.skills.open = !self.skills.open;
            if self.skills.open {
                self.skills.robot = None;
            }
            return true;
        }
        if !self.skills.open || self.skills.pending.is_some() {
            return false;
        }
        if let Some(i) = self
            .root
            .drop_down(cx, ids!(skill_sources))
            .selected(actions)
        {
            self.skills.index = i.checked_sub(1);
            return true;
        }
        for (id, op) in [
            (ids!(skill_use) as &[LiveId], "select"),
            (ids!(skill_clear), "clear"),
            (ids!(skill_refresh), "list"),
        ] {
            if self.root.button(cx, id).clicked(actions) {
                let pin = self
                    .skills
                    .index
                    .and_then(|i| self.skills.catalog["entries"].get(i))
                    .cloned();
                self.catalog_operation(w.robot, op, pin.filter(|_| op == "select"));
                return true;
            }
        }
        false
    }
}
