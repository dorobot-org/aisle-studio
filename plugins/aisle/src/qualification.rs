//! Register held-out suites and review their replayed Nexus measurements.
use super::*;
use dorobot_studio_core::runtime;
use serde_json::Value;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Qualification {
    pub open: bool,
    robot: Option<Robot>,
    catalog: Value,
    pending: Option<mpsc::Receiver<Result<Value, String>>>,
    operation: String,
    message: String,
    next_progress: Option<Instant>,
    progress: Option<(usize, usize)>,
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

impl Panel {
    fn qualification_op(
        &mut self,
        cx: &mut Cx,
        w: &Workbench,
        op: &'static str,
        id: Option<String>,
    ) {
        let robot = w.robot;
        let root = runtime::root();
        let state = runtime::user_data().join("aisle/qualifications");
        let controller = w.prepared.clone();
        let name = self.root.text_input(cx, ids!(qa_reviewer)).text();
        let reason = self.root.text_input(cx, ids!(qa_reason)).text();
        let expected = s(&self.qualification.catalog, "verdict_sha256").to_owned();
        let (tx, rx) = mpsc::channel();
        self.qualification.pending = Some(rx);
        self.qualification.operation = op.into();
        self.qualification.message.clear();
        std::thread::spawn(move || {
            let result = (|| {
                let mut cmd = dorobot_studio_core::api::aisle::python_command(&root)
                    .map_err(|e| e.to_string())?;
                cmd.arg(root.join("tools/skill_qualification/review.py"))
                    .arg(op)
                    .args(["--robot", robot.name(), "--state"])
                    .arg(state);
                if let Some(id) = id {
                    cmd.arg("--id").arg(id);
                }
                if let Some(path) = controller {
                    cmd.arg("--controller").arg(path);
                }
                if matches!(op, "approve" | "reject" | "handoff") {
                    cmd.arg("--reviewer")
                        .arg(name)
                        .arg("--rationale")
                        .arg(reason)
                        .arg("--expected")
                        .arg(expected);
                }
                let out = cmd.output().map_err(|e| e.to_string())?;
                if !out.status.success() {
                    return Err(String::from_utf8_lossy(&out.stderr)
                        .lines()
                        .last()
                        .unwrap_or("Qualification worker stopped")
                        .trim_start_matches("ValueError: ")
                        .to_owned());
                }
                serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
            })();
            let _ = tx.send(result);
            SignalToUI::set_ui_signal();
        });
    }

    pub(super) fn qualification_sync(&mut self, cx: &mut Cx, w: &Workbench) {
        if self.qualification.robot != Some(w.robot) {
            self.qualification.robot = Some(w.robot);
            self.qualification.catalog = Value::Null;
            self.qualification.progress = None;
            self.qualification_op(cx, w, "list", None);
        }
        let reply = self
            .qualification
            .pending
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(v) => Some(v),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err("Qualification worker stopped".into())),
            });
        if let Some(reply) = reply {
            self.qualification.pending = None;
            self.qualification.progress = None;
            match reply {
                Ok(v) => {
                    let ids = v["ids"].as_array().cloned().unwrap_or_default();
                    let labels = if ids.is_empty() {
                        vec!["No registrations".into()]
                    } else {
                        ids.iter()
                            .map(|id| {
                                format!(
                                    "{} · {}",
                                    w.robot.name(),
                                    id.as_str()
                                        .unwrap_or("")
                                        .chars()
                                        .take(16)
                                        .collect::<String>()
                                )
                            })
                            .collect()
                    };
                    let dd = self.root.drop_down(cx, ids!(qa_selector));
                    dd.set_labels(cx, labels);
                    dd.set_selected_item(
                        cx,
                        ids.iter().position(|id| id == &v["selected"]).unwrap_or(0),
                    );
                    if let Some(file) = v["exported"].as_str() {
                        self.qualification.message = format!(
                            "Saved {}",
                            runtime::user_data()
                                .join("aisle/qualifications")
                                .join(s(&v, "selected"))
                                .join(file)
                                .display()
                        );
                    }
                    self.qualification.catalog = v;
                }
                Err(e) => {
                    self.qualification.message = e;
                }
            }
        }
        let v = &self.qualification.catalog;
        let pending = self.qualification.pending.is_some();
        let selected = s(v, "selected");
        let reg = !selected.is_empty();
        let scope = if w.robot == Robot::Nano {
            "Nano · three floor cubes to the coffee table · 45 of 50 hidden layouts · zero guard bypasses"
        } else {
            "G1 · standing reach under bounded impacts · 8 corners and 12 sealed interior cases · all 20 must pass"
        };
        self.label(cx, ids!(qa_scope), scope);
        let pin = &v["package"];
        let ident = if reg {
            format!(
                "{} {} · {}\nController {}\nRegistration {}",
                s(pin, "name"),
                s(pin, "version"),
                s(pin, "source"),
                s(v, "controller_sha256"),
                selected
            )
        } else {
            "Choose an Octos skill in Task and prepare the reference controller to register it."
                .into()
        };
        self.label(cx, ids!(qa_identity), &ident);
        if reg
            && self
                .qualification
                .next_progress
                .is_none_or(|t| Instant::now() >= t)
        {
            self.qualification.next_progress = Some(Instant::now() + Duration::from_secs(1));
            let dir = runtime::user_data()
                .join("aisle/qualifications")
                .join(selected);
            if let Ok(files) = dir.read_dir() {
                let mut counts = (0, 0);
                for file in files.flatten() {
                    let name = file.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with("case-") && name.ends_with(".log") {
                        counts.0 += 1;
                    }
                    if name.starts_with("attempt-") && name.ends_with(".json") {
                        counts.1 += 1;
                    }
                }
                self.qualification.progress = Some(counts);
            }
        }
        let r = &v["report"];
        let cases = v["cases"].as_u64().unwrap_or(0) as usize;
        let (started, finished) = self.qualification.progress.unwrap_or((
            v["started"].as_u64().unwrap_or(0) as usize,
            v["finished"].as_u64().unwrap_or(0) as usize,
        ));
        let result = if r.is_object() {
            format!(
                "{} / {} passed · {} verified · {} guard bypasses · {}\nFailed case indices {}",
                r["successes"],
                r["cases"],
                r["verified"],
                r["guard_bypasses"],
                if r["qualified"] == true {
                    "Simulation gate met"
                } else {
                    "Below simulation gate"
                },
                r["failed_cases"]
            )
        } else if reg {
            format!("{finished} of {cases} finished · {started} started")
        } else {
            String::new()
        };
        self.label(cx, ids!(qa_result), &result);
        let failures = v["failures"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| format!("Case {} · {}", r["index"], s(r, "reason")))
            .collect::<Vec<_>>()
            .join("\n");
        self.label(cx, ids!(qa_failures), &failures);
        self.root
            .widget(cx, ids!(qa_failure_scroll))
            .set_visible(cx, !failures.is_empty());
        let review = &v["latest_review"];
        let decision = if review.is_object() {
            format!(
                "Signed {} · {} · {}\n{}",
                s(review, "disposition"),
                s(review, "reviewer"),
                s(review, "at"),
                s(review, "rationale")
            )
        } else if r.is_object() {
            "Awaiting a trusted reviewer’s signature".into()
        } else {
            String::new()
        };
        self.label(cx, ids!(qa_decision), &decision);
        self.label(
            cx,
            ids!(qa_status),
            if pending {
                match self.qualification.operation.as_str() {
                    "run" => "Executing registered cases in Nexus…",
                    "verify" | "approve" | "reject" | "handoff" => {
                        "Replaying measurements and checking package receipts…"
                    }
                    _ => "Loading qualification…",
                }
            } else {
                &self.qualification.message
            },
        );
        for (id, enabled) in [
            (
                ids!(qa_register) as &[LiveId],
                w.prepared.is_some() && w.plan["spec_skill"].is_object(),
            ),
            (ids!(qa_run), reg && started < cases),
            (ids!(qa_refresh), true),
            (ids!(qa_verify), reg && started == cases),
            (ids!(qa_approve), r["qualified"] == true),
            (ids!(qa_reject), r.is_object()),
            (
                ids!(qa_export),
                w.prepared.is_some()
                    && r["qualified"] == true
                    && s(review, "disposition") == "approve",
            ),
        ] {
            self.root
                .button(cx, id)
                .set_enabled(cx, enabled && !pending);
        }
    }

    pub(super) fn qualification_actions(
        &mut self,
        cx: &mut Cx,
        actions: &Actions,
        w: &Workbench,
    ) -> bool {
        if self.root.button(cx, ids!(qa_mode)).clicked(actions) {
            self.qualification.open = !self.qualification.open;
            self.trial_catalog_open = false;
            if self.qualification.open && self.qualification.pending.is_none() {
                self.qualification.robot = None;
            }
            return true;
        }
        if !self.qualification.open || self.qualification.pending.is_some() {
            return false;
        }
        let v = &self.qualification.catalog;
        if let Some(i) = self.root.drop_down(cx, ids!(qa_selector)).selected(actions) {
            let id = v["ids"].get(i).and_then(Value::as_str).map(str::to_owned);
            self.qualification_op(cx, w, "list", id);
            return true;
        }
        for (id, op) in [
            (ids!(qa_register) as &[LiveId], "register"),
            (ids!(qa_run), "run"),
            (ids!(qa_verify), "verify"),
            (ids!(qa_refresh), "list"),
            (ids!(qa_approve), "approve"),
            (ids!(qa_reject), "reject"),
            (ids!(qa_export), "handoff"),
        ] {
            if self.root.button(cx, id).clicked(actions) {
                let id = v["selected"].as_str().map(str::to_owned);
                self.qualification_op(cx, w, op, id);
                return true;
            }
        }
        false
    }
}
