use super::*;

impl Panel {
    pub(super) fn device_panel(&self, cx: &mut Cx, w: &Workbench, setup: bool) {
        let g1 = w.evidence["robot"] == "G1";
        let standing =
            g1 && (!setup || w.robot == Robot::G1) && w.evidence["kind"] == "standing-inspection";
        let matching = w.evidence["robot"] == w.robot.name();
        let selected = w.evidence["task_success"] == true && (!setup || matching);
        self.root
            .widget(cx, ids!(run_preflight_trial))
            .set_visible(cx, setup && !selected);
        self.label(
            cx,
            ids!(context_label),
            if setup {
                "Hardware preflight   /   Read-only adapter check"
            } else {
                "Selected simulation trial   /   Device preflight"
            },
        );
        self.label(
            cx,
            ids!(device_title),
            if setup && !selected {
                "Run a successful simulation for this robot"
            } else if !setup && !w.preflight.is_object() {
                "No preflight recorded"
            } else if g1 {
                "Unitree G1 · 29-joint LowState"
            } else if selected {
                "Adora Nano · Feetech base"
            } else {
                "Select a successful simulation trial"
            },
        );
        let instructions = if !setup {
            "Open Robot → Hardware preflight to check an adapter for the selected robot."
        } else if !selected {
            "Select a successful trial for this robot in Evidence or run a simulation."
        } else if standing {
            "Replay encoders and IMU from the selected simulation or observe an external DDS publisher. Motor output stays disabled."
        } else if g1 {
            "Select a standing-reach trial to run the G1 telemetry preflight."
        } else {
            "Check which measurements the installed Feetech adapter provides for this controller."
        };
        self.label(cx, ids!(device_info), instructions);
        self.root
            .widget(cx, ids!(device_endpoint))
            .set_visible(cx, setup && standing);
        self.root
            .widget(cx, ids!(device_listen))
            .set_visible(cx, setup && standing);
        self.root
            .widget(cx, ids!(device_stop))
            .set_visible(cx, setup && w.preflighting());
        self.root.button(cx, ids!(device_replay)).set_text(
            cx,
            if g1 {
                "Replay through DDS"
            } else {
                "Check adapter"
            },
        );
        self.root
            .button(cx, ids!(device_replay))
            .set_enabled(cx, !w.busy() && selected && (standing || !g1));
        self.root
            .button(cx, ids!(device_listen))
            .set_enabled(cx, !w.busy() && selected && standing);
        self.root
            .widget(cx, ids!(device_replay))
            .set_visible(cx, setup && selected);
        let v = if setup && !matching {
            &serde_json::Value::Null
        } else {
            &w.preflight
        };
        let summary = match v["state"].as_str() {
            Some("Dry run measured") => {
                let mut lines = vec![
                    format!(
                        "{} · {} measurements",
                        v["source_kind"].as_str().unwrap_or("DDS"),
                        v["samples"]
                    ),
                    format!(
                        "{} joints · {} dry-run steps · 0 motor commands sent",
                        v["joints"], v["dry_run_steps"]
                    ),
                    format!(
                        "Longest gaps · source {:.1} ms · receiver {:.1} ms",
                        v["max_source_gap_ms"].as_f64().unwrap_or_default(),
                        v["max_receive_gap_ms"].as_f64().unwrap_or_default()
                    ),
                    format!(
                        "Peak target to measured joint difference {:.3} rad",
                        v["peak_target_feedback_difference_rad"]
                            .as_f64()
                            .unwrap_or_default()
                    ),
                ];
                if let Some(difference) = v["max_nexus_target_difference_rad"].as_f64() {
                    lines.push(format!(
                        "Maximum paired target difference {difference:.6} rad"
                    ));
                }
                lines.push("Physical device qualification pending".into());
                lines.join("\n")
            }
            Some(state) => format!("{state}\n{}", v["reason"].as_str().unwrap_or("")),
            None => String::new(),
        };
        let summary = if let Some(pin) = v["spec_simulation"]["package"]["package_sha256"].as_str()
        {
            format!(
                "{summary}\nSimulation skill verified · {} · {pin}",
                v["spec_simulation"]["package"]["source"]
                    .as_str()
                    .unwrap_or("")
            )
        } else {
            summary
        };
        self.label(cx, ids!(device_result), &summary);
        self.root
            .button(cx, ids!(device_folder))
            .set_enabled(cx, !w.busy() && v.is_object());
    }
}
