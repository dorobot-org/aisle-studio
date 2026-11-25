//! Register held-out suites and review their replayed Nexus measurements.
use super::*;
use dorobot_studio_core::runtime;
use serde_json::Value;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

struct QualificationReply {
    data: Value,
    frame: Vec<u8>,
}

#[derive(Default)]
pub(super) struct Qualification {
    pub open: bool,
    robot: Option<Robot>,
    catalog: Value,
    pending: Option<mpsc::Receiver<Result<QualificationReply, String>>>,
    operation: String,
    message: String,
    next_progress: Option<Instant>,
    progress: Option<(usize, usize)>,
    failed_index: usize,
    loaded_comparison: Option<(String, u64)>,
    comparison: Value,
    inspecting: bool,
    case_index: usize,
    exposure_index: usize,
    inspected: Value,
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

fn replay_folder(id: &str, index: u64) -> PathBuf {
    runtime::user_data()
        .join("aisle/case-replays")
        .join(id)
        .join(format!("case-{index:03}"))
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
        let case = self.qualification.catalog["failures"]
            .get(self.qualification.failed_index)
            .and_then(|r| r["index"].as_u64());
        let case_index = self.qualification.case_index;
        let exposure_index = self.qualification.exposure_index;
        let (tx, rx) = mpsc::channel();
        self.qualification.pending = Some(rx);
        self.qualification.operation = op.into();
        self.qualification.message.clear();
        std::thread::spawn(move || {
            let result = (|| {
                let mut cmd = dorobot_studio_core::api::aisle::python_command(&root)
                    .map_err(|e| e.to_string())?;
                if op == "inspect" {
                    let id = id.as_deref().ok_or("Select a verified qualification")?;
                    cmd.arg(root.join("tools/skill_qualification/inspect_case.py"))
                        .arg("--state")
                        .arg(&state)
                        .args(["--id", id, "--expected-verdict", &expected, "--case"])
                        .arg(case_index.to_string())
                        .arg("--exposure")
                        .arg(exposure_index.to_string());
                } else if op == "replay" {
                    let id = id.as_deref().ok_or("Choose a disclosed registration")?;
                    let index = case.ok_or("Choose a failed layout")?;
                    let base = state.join(id);
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_nanos();
                    let out =
                        replay_folder(id, index).join(format!("{stamp}-{}", std::process::id()));
                    cmd.arg(root.join("tools/skill_qualification/replay_case.py"))
                        .arg("--baseline")
                        .arg(&base)
                        .arg("--published-verdict")
                        .arg(base.join("verification.json"))
                        .arg("--expected-verdict")
                        .arg(&expected)
                        .arg("--case")
                        .arg(index.to_string())
                        .arg("--out")
                        .arg(out)
                        .arg("--remember");
                } else {
                    cmd.arg(root.join("tools/skill_qualification/review.py"))
                        .arg(op)
                        .args(["--robot", robot.name(), "--state"])
                        .arg(state);
                    if let Some(id) = id {
                        cmd.arg("--id").arg(id);
                    }
                }
                if let Some(path) = controller.filter(|_| op != "inspect") {
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
                let mut payload = out.stdout;
                let frame = if op == "inspect" {
                    let header = payload
                        .iter()
                        .position(|b| *b == b'\n')
                        .ok_or("The evidence reader returned an incomplete response")?;
                    payload.split_off(header + 1)
                } else {
                    Vec::new()
                };
                let data: Value = serde_json::from_slice(&payload).map_err(|e| e.to_string())?;
                if op == "inspect" {
                    let width = data["width"].as_u64().unwrap_or(0);
                    let height = data["height"].as_u64().unwrap_or(0);
                    if width.checked_mul(height).and_then(|n| n.checked_mul(3))
                        != Some(frame.len() as u64)
                    {
                        return Err("The RGB view differs from its recorded dimensions".into());
                    }
                }
                Ok(QualificationReply { data, frame })
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
                Ok(reply) if self.qualification.operation == "inspect" => {
                    let v = reply.data;
                    if let Some(mut player) = self
                        .root
                        .video_player(cx, ids!(qa_exposure_rgb))
                        .borrow_mut()
                    {
                        player.clear(cx);
                        if !reply.frame.is_empty() {
                            player.set_frame(
                                cx,
                                v["width"].as_u64().unwrap_or(0) as usize,
                                v["height"].as_u64().unwrap_or(0) as usize,
                                &reply.frame,
                            );
                        }
                    }
                    self.root
                        .drop_down(cx, ids!(qa_case_selector))
                        .set_selected_item(cx, self.qualification.case_index);
                    self.qualification.inspected = v;
                }
                Ok(reply) if self.qualification.operation == "replay" => {
                    self.qualification.comparison = reply.data;
                }
                Ok(reply) => {
                    let v = reply.data;
                    if v["verdict_sha256"] != self.qualification.catalog["verdict_sha256"]
                        || v["selected"] != self.qualification.catalog["selected"]
                    {
                        self.qualification.inspecting = false;
                        self.qualification.inspected = Value::Null;
                        self.qualification.case_index = 0;
                        self.qualification.exposure_index = 0;
                        let cases = v["report"]["cases"].as_u64().unwrap_or(0);
                        let selector = self.root.drop_down(cx, ids!(qa_case_selector));
                        let failures = v["failures"].as_array();
                        selector.set_labels(
                            cx,
                            (0..cases)
                                .map(|i| {
                                    let failed = failures.is_some_and(|rows| {
                                        rows.iter().any(|row| row["index"].as_u64() == Some(i))
                                    });
                                    format!("{i} · {}", if failed { "failed" } else { "passed" })
                                })
                                .collect(),
                        );
                        selector.set_selected_item(cx, 0);
                        self.qualification.failed_index = 0;
                    }
                    let failures = v["failures"].as_array().cloned().unwrap_or_default();
                    let failed = self.root.drop_down(cx, ids!(qa_cases));
                    failed.set_labels(
                        cx,
                        if failures.is_empty() {
                            vec!["No failed layouts".into()]
                        } else {
                            failures
                                .iter()
                                .map(|r| {
                                    format!(
                                        "Case {} · {}",
                                        r["index"],
                                        s(r, "reason").trim_start_matches("ValueError: ")
                                    )
                                })
                                .collect()
                        },
                    );
                    failed.set_selected_item(cx, self.qualification.failed_index);
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
            let inputs = v["sensors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|value| match value {
                    "onboard_rgb" => "onboard RGB",
                    "imu" => "IMU",
                    "joint_encoders" => "joint encoders",
                    v => v,
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} {} · {} · Nexus · {}\nPackage {}\nController {}\nRegistration {}",
                s(pin, "name"),
                s(pin, "version"),
                s(pin, "source"),
                inputs,
                s(pin, "package_sha256"),
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
                "{} / {} passed · {} verified · {} guard bypasses · {}",
                r["successes"],
                r["cases"],
                r["verified"],
                r["guard_bypasses"],
                if r["qualified"] == true {
                    "Simulation gate met"
                } else {
                    "Below simulation gate"
                }
            )
        } else if reg {
            format!("{finished} of {cases} finished · {started} started")
        } else {
            String::new()
        };
        self.label(cx, ids!(qa_result), &result);
        self.update_case_inspector(cx, r.is_object(), w.robot);
        let v = &self.qualification.catalog;
        let r = &v["report"];
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
            .set_visible(cx, !failures.is_empty() && w.robot != Robot::Nano);
        let selected_case = v["failures"]
            .get(self.qualification.failed_index)
            .and_then(|row| row["index"].as_u64());
        let development = w.robot == Robot::Nano && selected_case.is_some();
        self.root
            .widget(cx, ids!(qa_development))
            .set_visible(cx, development);
        if let Some(index) = selected_case.filter(|_| development) {
            let key = (selected.to_owned(), index);
            if self.qualification.loaded_comparison.as_ref() != Some(&key) {
                self.qualification.loaded_comparison = Some(key);
                self.qualification.comparison =
                    std::fs::read(replay_folder(selected, index).join("latest.json"))
                        .ok()
                        .and_then(|raw| serde_json::from_slice(&raw).ok())
                        .unwrap_or_default();
            }
        }
        let c = &self.qualification.comparison;
        let show = development
            && s(c, "original_registration") == selected
            && c["case"].as_u64() == selected_case;
        self.root
            .widget(cx, ids!(qa_comparison))
            .set_visible(cx, show);
        if show {
            let scored = |passed: &Value| if passed == true { "passed" } else { "failed" };
            self.label(cx,ids!(qa_comparison),&format!("Case {} · baseline {} → candidate {} · {} scored ticks · {} guard bypasses\nCandidate {}",
                c["case"],scored(&c["baseline_passed"]),scored(&c["candidate_passed"]),c["physics_ticks"],c["bypasses"],s(c,"candidate_controller_sha256")));
        }
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
        self.root
            .widget(cx, ids!(qa_review_card))
            .set_visible(cx, reg);
        self.root
            .widget(cx, ids!(qa_export))
            .set_visible(cx, s(review, "disposition") == "approve");
        self.root
            .widget(cx, ids!(qa_run))
            .set_visible(cx, reg && started < cases);
        self.label(
            cx,
            ids!(qa_status),
            if pending {
                match self.qualification.operation.as_str() {
                    "run" => "Executing registered cases in Nexus…",
                    "replay" => "Testing the selected controller on the disclosed layout in Nexus…",
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
            (
                ids!(qa_replay_case),
                development
                    && w.prepared.is_some()
                    && w.plan["kind"] == "table-transfer"
                    && w.plan["spec_skill"].is_object(),
            ),
            (ids!(qa_refresh), true),
            (ids!(qa_verify), reg && started == cases),
            (ids!(qa_approve), r["qualified"] == true),
            (ids!(qa_reject), r.is_object()),
            (
                ids!(qa_export),
                w.prepared_digest() == Some(s(v, "controller_sha256"))
                    && r["qualified"] == true
                    && s(review, "disposition") == "approve",
            ),
        ] {
            self.root
                .button(cx, id)
                .set_enabled(cx, enabled && !pending);
        }
    }

    fn update_case_inspector(&self, cx: &mut Cx, has_report: bool, robot: Robot) {
        let qa = &self.qualification;
        self.root
            .widget(cx, ids!(qa_inspect))
            .set_visible(cx, has_report);
        self.root
            .button(cx, ids!(qa_inspect))
            .set_enabled(cx, qa.pending.is_none());
        self.root.button(cx, ids!(qa_inspect)).set_text(
            cx,
            if qa.inspecting {
                "Close case evidence"
            } else {
                "Inspect case evidence"
            },
        );
        self.root
            .widget(cx, ids!(qa_inspector))
            .set_visible(cx, has_report && qa.inspecting);
        if !has_report || !qa.inspecting {
            return;
        }
        let v = &qa.inspected;
        self.label(cx, ids!(qa_case_coverage), s(v, "coverage"));
        self.label(
            cx,
            ids!(qa_case_summary),
            if v.is_object() {
                s(v, "summary")
            } else if qa.pending.is_some() {
                "Reading the recorded case…"
            } else {
                &qa.message
            },
        );
        let n = v["exposures"].as_u64().unwrap_or(0) as usize;
        self.root
            .widget(cx, ids!(qa_exposures))
            .set_visible(cx, n > 0);
        self.root
            .widget(cx, ids!(qa_exposure_rgb))
            .set_visible(cx, n > 0);
        self.root
            .widget(cx, ids!(qa_exposure_caption))
            .set_visible(cx, robot == Robot::Nano && v.is_object());
        if n == 0 {
            self.label(
                cx,
                ids!(qa_exposure_caption),
                "No recorded release exposures for this case",
            );
        } else {
            self.label(
                cx,
                ids!(qa_exposure_count),
                &format!("Exposure {} of {n}", qa.exposure_index + 1),
            );
            self.label(
                cx,
                ids!(qa_exposure_caption),
                &format!(
                    "{}\nCamera order · head, left wrist, right wrist · RGB {}",
                    s(v, "caption"),
                    s(v, "frame_sha256").get(..12).unwrap_or("")
                ),
            );
        }
        self.root
            .button(cx, ids!(qa_previous_exposure))
            .set_enabled(cx, qa.pending.is_none() && qa.exposure_index > 0);
        self.root
            .button(cx, ids!(qa_next_exposure))
            .set_enabled(cx, qa.pending.is_none() && qa.exposure_index + 1 < n);
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
        let inspect_button = self.root.button(cx, ids!(qa_inspect)).clicked(actions);
        if inspect_button {
            self.qualification.inspecting = !self.qualification.inspecting;
            if !self.qualification.inspecting {
                return true;
            }
        }
        if self.qualification.inspecting {
            let selected = self
                .root
                .drop_down(cx, ids!(qa_case_selector))
                .selected(actions);
            let prev = self
                .root
                .button(cx, ids!(qa_previous_exposure))
                .clicked(actions);
            let next = self
                .root
                .button(cx, ids!(qa_next_exposure))
                .clicked(actions);
            if let Some(index) = selected {
                self.qualification.case_index = index;
                self.qualification.exposure_index = 0;
            } else if prev {
                self.qualification.exposure_index =
                    self.qualification.exposure_index.saturating_sub(1);
            } else if next {
                self.qualification.exposure_index += 1;
            }
            if inspect_button || selected.is_some() || prev || next {
                let id = v["selected"].as_str().map(str::to_owned);
                self.qualification.inspected = Value::Null;
                self.qualification_op(cx, w, "inspect", id);
                return true;
            }
        }
        if let Some(i) = self.root.drop_down(cx, ids!(qa_selector)).selected(actions) {
            let id = v["ids"].get(i).and_then(Value::as_str).map(str::to_owned);
            self.qualification_op(cx, w, "list", id);
            return true;
        }
        if let Some(i) = self.root.drop_down(cx, ids!(qa_cases)).selected(actions) {
            self.qualification.failed_index = i;
            return true;
        }
        for (id, op) in [
            (ids!(qa_replay_case) as &[LiveId], "replay"),
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
