use dorobot_studio_core::{
    api::{
        aisle::{python_command, Workbench},
        experiment_replay::{catalog, read, Entry, Replay},
    },
    runtime,
    widgets::video_player::VideoPlayerWidgetRefExt,
};
use dorobot_studio_plugin::*;
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    mod.widgets.TrialEvidenceBase = #(TrialEvidence::register_widget(vm))
    let Line=View{width:Fill height:Fit flow:Right spacing:6 align:Align{y:0.5}}
    let Txt=Label{width:Fill height:Fit max_lines:6}
    let Tool=mod.widgets.ux.ToolButton{}
    let Field=TextInput{width:Fill height:34}
    mod.widgets.TrialEvidence=set_type_default() do mod.widgets.TrialEvidenceBase{
        width:Fill height:Fill flow:Down spacing:6
        Line{ev_search := Field{empty_text:"Find a run, scene, task, robot, tag or outcome"} ev_filter := Tool{text:"Search"} ev_refresh := Tool{text:"Refresh"}}
        Line{ev_entries := DropDown{width:Fill} ev_identity := Txt{width:Fill}}
        View{width:Fill height:Fill flow:Right spacing:12
            View{width:Fill height:Fill flow:Down spacing:6
                Line{ev_camera := DropDown{width:160} ev_clock := Txt{}}
                ev_image := VideoPlayer{width:Fill height:Fill}
                ev_scrub := Slider{width:Fill height:34 min:0 max:1}
                Line{ev_play := Tool{text:"Play"} ev_back := Tool{text:"Previous frame"} ev_forward := Tool{text:"Next frame"} ev_phase := DropDown{width:Fill}}
                ev_trace := Txt{} ev_channels := Txt{} ev_files := Txt{}
            }
            ev_inspector := ScrollYView{width:380 height:Fill flow:Down spacing:7
                ev_verdict := Txt{} ev_lineage := Txt{}
                Line{ev_parent := Tool{text:"Open parent"} ev_original := Tool{text:"Rerun original"}}
                Label{text:"Run notes"}
                Line{Label{width:62 text:"Name"} ev_name := Field{empty_text:"Run name"}}
                Line{Label{width:62 text:"Notes"} ev_notes := Field{empty_text:"Observation"}}
                Line{Label{width:62 text:"Tags"} ev_tags := Field{empty_text:"Comma-separated tags"}}
                Line{Label{width:62 text:"Metadata"} ev_fields := Field{empty_text:"JSON object"}}
                Line{ev_save_notes := Tool{text:"Save notes"} ev_revisions := DropDown{width:Fill}}
                ev_revision_detail := Txt{}
                Label{text:"Derived configuration"} ev_variant_name := Field{empty_text:"Variant name"}
                Line{ev_object := DropDown{width:Fill} ev_axis := DropDown{width:64} ev_position := Field{empty_text:"Metres"}}
                Line{Label{text:"Seeds"} ev_seed := Field{empty_text:"One per repetition"} ev_save_variant := Tool{text:"Save As"}}
                ev_options := Tool{text:"Variant options"}
                ev_options_body := View{visible:false width:Fill height:Fit flow:Down spacing:6
                    ev_task_source := DropDown{width:Fill}
                    ev_logs := DropDown{width:Fill} ev_capture := DropDown{width:Fill}
                    Label{text:"Runtime features (JSON)"} ev_params := Field{}
                }
                ev_variants := DropDown{width:Fill}
                ev_diff := Txt{}
                ev_edited := mod.widgets.ux.PrimaryButton{text:"Run edited version"}
                Label{text:"Agent history"} ev_events := DropDown{width:Fill} ev_event_detail := Txt{max_lines:14}
            }
        }
        ev_feedback := Txt{}
    }
}
#[derive(Script, ScriptHook, Widget)]
pub struct TrialEvidence {
    #[deref]
    view: View,
    #[rust]
    loaded: bool,
    #[rust]
    prepared: Option<(std::path::PathBuf, String)>,
    #[rust]
    options_open: bool,
    #[rust]
    entries: Vec<Entry>,
    #[rust]
    visible_entries: Vec<usize>,
    #[rust]
    current: Option<usize>,
    #[rust]
    clip: Option<Replay>,
    #[rust]
    cursor: usize,
    #[rust]
    playing: Option<Instant>,
    #[rust]
    layout: Value,
    #[rust]
    variants: Vec<Value>,
    #[rust]
    children: Vec<Child>,
    #[rust]
    next_poll: Option<Instant>,
}
type Res<T> = Result<T, String>;
fn msg(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn file_or_null(path: &Path) -> Value {
    read(path).unwrap_or(Value::Null)
}
fn pose_text(value: &Value) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .map(|n| format!("{n:.3}"))
        .collect::<Vec<_>>()
        .join(", ")
}
fn channel(row: &Value, key: &str) -> &'static str {
    if row["recording"][key] != true {
        return "off";
    }
    match row["channels"][key]["status"].as_str() {
        Some("partial") => "partial",
        Some("complete") => "recorded",
        _ if row["channels"][key]["present"] == true => "recorded",
        _ => "unavailable",
    }
}
impl TrialEvidence {
    fn label(&self, cx: &mut Cx, id: &[LiveId], text: &str) {
        self.view.label(cx, id).set_text(cx, text);
    }
    fn field(&self, cx: &Cx, id: &[LiveId]) -> String {
        self.view.text_input(cx, id).text()
    }
    fn set(&self, cx: &mut Cx, id: &[LiveId], text: &str) {
        self.view.text_input(cx, id).set_text(cx, text);
    }
    fn index(&self, cx: &Cx, id: &[LiveId]) -> usize {
        self.view.drop_down(cx, id).selected_item()
    }
    fn items(&self, cx: &mut Cx, id: &[LiveId], list: Vec<String>, index: usize) {
        let drop = self.view.drop_down(cx, id);
        drop.set_labels(cx, list);
        drop.set_selected_item(cx, index);
    }
    fn entry(&self) -> Res<&Entry> {
        self.current
            .and_then(|i| self.entries.get(i))
            .ok_or("Select an experiment trial".into())
    }
    fn load(&mut self, cx: &mut Cx, id: Option<&str>) -> Res<()> {
        self.entries = catalog(&runtime::user_data().join("aisle/experiments")).map_err(msg)?;
        let filter = self.field(cx, ids!(ev_search));
        self.visible_entries = (0..self.entries.len())
            .filter(|&i| self.entries[i].matches(&filter))
            .collect();
        let selected = id
            .and_then(|id| {
                self.visible_entries
                    .iter()
                    .position(|&i| self.entries[i].id() == id)
            })
            .unwrap_or(0);
        self.items(
            cx,
            ids!(ev_entries),
            self.visible_entries
                .iter()
                .map(|&i| self.entries[i].label())
                .collect(),
            selected,
        );
        self.select(cx, selected)
    }
    fn select(&mut self, cx: &mut Cx, index: usize) -> Res<()> {
        self.current = self.visible_entries.get(index).copied();
        self.clip = None;
        self.cursor = 0;
        self.playing = None;
        VideoPlayerWidgetRefExt::as_video_player(&self.view.widget(cx, ids!(ev_image))).clear(cx);
        self.view.button(cx, ids!(ev_play)).set_text(cx, "Play");
        self.view
            .widget(cx, ids!(ev_inspector))
            .set_visible(cx, self.current.is_some());
        let Some(i) = self.current else {
            for id in [
                ids!(ev_identity) as &[LiveId],
                ids!(ev_lineage),
                ids!(ev_verdict),
                ids!(ev_trace),
                ids!(ev_channels),
                ids!(ev_files),
                ids!(ev_diff),
                ids!(ev_clock),
                ids!(ev_event_detail),
                ids!(ev_revision_detail),
            ] {
                self.label(cx, id, "");
            }
            self.label(cx, ids!(ev_feedback), "No matching trials");
            return Ok(());
        };
        let e = &self.entries[i];
        let r = &e.row;
        self.label(
            cx,
            ids!(ev_identity),
            &format!(
                "{} · {} · {} · seed {}",
                e.scene_name,
                r["robot"].as_str().unwrap_or(""),
                "Nexus",
                r["seed"]
            ),
        );
        self.label(
            cx,
            ids!(ev_verdict),
            &format!(
                "{}  /  {}\nRun {}\n{}  ·  scene v{}\n{}",
                e.batch_name,
                r["status"].as_str().unwrap_or(""),
                e.id(),
                r["outcome"]["physics_ticks"]
                    .as_u64()
                    .map(|n| format!("{n} physics steps"))
                    .unwrap_or_else(|| "Awaiting measured result".into()),
                r["scene"]["revision"],
                r["reason"].as_str().unwrap_or("")
            ),
        );
        self.label(cx, ids!(ev_lineage), &self.comparison(e));
        self.view
            .button(cx, ids!(ev_parent))
            .set_enabled(cx, r["parent_run_id"].is_string());
        self.layout = file_or_null(&e.trial().join("sampled-layout.json"));
        for (id, key) in [
            (ids!(ev_name) as &[LiveId], "name"),
            (ids!(ev_notes), "notes"),
        ] {
            self.set(cx, id, e.notes[key].as_str().unwrap_or(""));
        }
        self.set(
            cx,
            ids!(ev_tags),
            &e.notes["tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        );
        self.set(cx, ids!(ev_fields), &e.notes["fields"].to_string());
        self.set(
            cx,
            ids!(ev_variant_name),
            &format!("{} variant", e.notes["name"].as_str().unwrap_or("Trial")),
        );
        self.set(cx, ids!(ev_seed), &r["seed"].to_string());
        let cfg = file_or_null(&e.batch.join("batch.json"))["configuration"].clone();
        self.set(cx, ids!(ev_params), &cfg["parameters"].to_string());
        self.items(
            cx,
            ids!(ev_task_source),
            vec![
                "Parent scene, task and controller".into(),
                "Prepared selection from Task".into(),
            ],
            0,
        );
        let log_idx = usize::from(r["recording"]["robot_trace"] == true)
            + 2 * usize::from(r["recording"]["agent_history"] == true);
        self.items(
            cx,
            ids!(ev_logs),
            vec![
                "No optional logs".into(),
                "Robot trace".into(),
                "Agent history".into(),
                "Robot trace and agent history".into(),
            ],
            log_idx,
        );
        self.items(
            cx,
            ids!(ev_capture),
            vec!["Record cameras".into(), "Skip camera dataset".into()],
            usize::from(cfg["record_cameras"] == false),
        );
        let revisions = e.revisions().map_err(msg)?;
        self.items(
            cx,
            ids!(ev_revisions),
            (1..=revisions.len())
                .map(|n| format!("Notes v{n}"))
                .collect(),
            revisions.len().saturating_sub(1),
        );
        self.revision(cx)?;
        self.variants = e.variants().map_err(msg)?;
        self.items(
            cx,
            ids!(ev_variants),
            self.variants
                .iter()
                .map(|v| v["name"].as_str().unwrap_or("Variant").into())
                .collect(),
            self.variants.len().saturating_sub(1),
        );
        self.variant_detail(cx);
        self.items(
            cx,
            ids!(ev_object),
            self.layout["scene"]["instances"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item["name"].as_str().map(String::from))
                .collect(),
            0,
        );
        self.items(
            cx,
            ids!(ev_axis),
            vec!["X".into(), "Y".into(), "Z".into()],
            0,
        );
        self.position(cx);
        let clip = Replay::open(e).map_err(msg)?;
        self.items(
            cx,
            ids!(ev_camera),
            clip.cameras
                .iter()
                .map(|id| {
                    match id.as_str() {
                        "operator" => "Operator camera",
                        "head" => "Head",
                        "left_wrist" => "Left wrist",
                        "right_wrist" => "Right wrist",
                        other => other,
                    }
                    .to_string()
                })
                .collect(),
            0,
        );
        self.items(
            cx,
            ids!(ev_phase),
            clip.phases
                .iter()
                .map(|p| {
                    format!(
                        "{} · {} ms",
                        p["name"].as_str().unwrap_or(""),
                        p["tick"].as_u64().unwrap_or(0) * (clip.tick_ns / 1_000_000)
                    )
                })
                .collect(),
            0,
        );
        self.items(
            cx,
            ids!(ev_events),
            clip.events
                .iter()
                .map(|ev| {
                    format!(
                        "{} · {}",
                        ev["stage"].as_str().unwrap_or(""),
                        ev["event"].as_str().unwrap_or("")
                    )
                })
                .collect(),
            0,
        );
        self.label(
            cx,
            ids!(ev_channels),
            &format!(
                "RGB · {} frames per camera   /   Robot trace · {}   /   Agent history · {}",
                clip.camera_frames,
                channel(r, "robot_trace"),
                channel(r, "agent_history")
            ),
        );
        let artifacts = [
            (
                "robot-trace.jsonl",
                r["channels"]["robot_trace"]["present"] == true,
            ),
            (
                "agent-history.jsonl",
                r["channels"]["agent_history"]["present"] == true,
            ),
            ("result.json", r["outcome"].is_object()),
        ]
        .into_iter()
        .filter_map(|(name, present)| present.then_some(name))
        .collect::<Vec<_>>()
        .join(" · ");
        self.label(cx, ids!(ev_files), &artifacts);
        self.clip = Some(clip);
        self.history(cx);
        self.frame(cx)?;
        self.label(cx, ids!(ev_feedback), "");
        Ok(())
    }
    fn comparison(&self, e: &Entry) -> String {
        let Some(id) = e.row["parent_run_id"].as_str() else {
            return "Original execution".into();
        };
        let Some(p) = self.entries.iter().find(|p| p.id() == id) else {
            return format!("Parent {id} · unavailable");
        };
        let current = file_or_null(&e.trial().join("sampled-layout.json"));
        let prior = file_or_null(&p.trial().join("sampled-layout.json"));
        let mut lines = vec![
            format!(
                "Parent {} · {}  |  This run · {}",
                &id[..8],
                p.row["status"].as_str().unwrap_or(""),
                e.row["status"].as_str().unwrap_or("")
            ),
            format!(
                "Measured steps   {}  |  {}",
                p.row["outcome"]["physics_ticks"], e.row["outcome"]["physics_ticks"]
            ),
        ];
        for item in current["scene"]["instances"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(old) = prior["scene"]["instances"]
                .as_array()
                .and_then(|a| a.iter().find(|x| x["id"] == item["id"]))
            {
                if old["position_m"] != item["position_m"] {
                    lines.push(format!(
                        "{}   {} → {} m",
                        item["name"].as_str().unwrap_or("Object"),
                        pose_text(&old["position_m"]),
                        pose_text(&item["position_m"])
                    ));
                }
            }
        }
        let p_result = file_or_null(&p.trial().join("result.json"));
        let result = file_or_null(&e.trial().join("result.json"));
        if let Some(cubes) = result["cubes"].as_object() {
            for (color, cube) in cubes {
                if let (Some(old), Some(new)) = (
                    p_result["cubes"][color]["table_contact_tail_ticks"].as_u64(),
                    cube["table_contact_tail_ticks"].as_u64(),
                ) {
                    lines.push(format!("{color} tabletop contact · {old} | {new} ticks"));
                }
            }
        }
        if e.row["seed"] != p.row["seed"] {
            lines.push(format!("Seed {} → {}", p.row["seed"], e.row["seed"]));
        }
        lines.join("\n")
    }
    fn revision(&self, cx: &mut Cx) -> Res<()> {
        let e = self.entry()?;
        let revisions = e.revisions().map_err(msg)?;
        let v = revisions
            .get(self.index(cx, ids!(ev_revisions)))
            .map(|p| read(p).map_err(msg))
            .transpose()?;
        self.label(
            cx,
            ids!(ev_revision_detail),
            &v.map(|v| {
                format!(
                    "v{} · {} · {}\n{}",
                    v["revision"],
                    v["updated"].as_str().unwrap_or(""),
                    v["name"].as_str().unwrap_or(""),
                    v["notes"].as_str().unwrap_or("")
                )
            })
            .unwrap_or_else(|| "No saved note revisions".into()),
        );
        Ok(())
    }
    fn position(&self, cx: &mut Cx) {
        let obj = &self.layout["scene"]["instances"][self.index(cx, ids!(ev_object))];
        let value = obj["position_m"][self.index(cx, ids!(ev_axis))]
            .as_f64()
            .map(|v| v.to_string())
            .unwrap_or_default();
        self.set(cx, ids!(ev_position), &value);
    }
    fn variant_detail(&self, cx: &mut Cx) {
        let v = self.variants.get(self.index(cx, ids!(ev_variants)));
        self.view
            .button(cx, ids!(ev_edited))
            .set_enabled(cx, v.is_some());
        let mut lines = vec![];
        if let Some(v) = v {
            let c = &v["configuration"];
            lines.push(format!(
                "Seed {} · parent {}",
                c["seeds"][0],
                v["parent"]["run_id"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(8)
                    .collect::<String>()
            ));
            lines.push(format!(
                "{} repetitions · cameras {} · trace {} · agent history {}",
                c["repeat_count"],
                c["record_cameras"],
                c["recording"]["robot_trace"],
                c["recording"]["agent_history"]
            ));
            if v["controller_sha256"].is_string()
                && v["controller_sha256"] != v["parent"]["controller_sha256"]
            {
                lines.push(format!(
                    "Revised scene v{} · controller {}",
                    c["scene"]["revision"],
                    v["controller_sha256"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(10)
                        .collect::<String>()
                ));
            }
            for (id, axes) in c["pose_edits"].as_object().into_iter().flatten() {
                if let Some(item) = self.layout["scene"]["instances"]
                    .as_array()
                    .and_then(|s| s.iter().find(|i| i["id"] == *id))
                {
                    for (axis, value) in axes.as_object().into_iter().flatten() {
                        let i = "xyz".find(axis).unwrap_or(0);
                        lines.push(format!(
                            "{} {}   {} → {} m",
                            item["name"].as_str().unwrap_or(id),
                            axis,
                            item["position_m"][i],
                            value
                        ));
                    }
                }
            }
        }
        self.label(cx, ids!(ev_diff), &lines.join("\n"));
    }
    fn history(&self, cx: &mut Cx) {
        let ev = self
            .clip
            .as_ref()
            .and_then(|c| c.events.get(self.index(cx, ids!(ev_events))));
        let detail = ev
            .map(|v| {
                format!(
                    "{} · {} · {}\n{}\n{}",
                    v["provider"].as_str().unwrap_or(""),
                    v["model"].as_str().unwrap_or(""),
                    v["timestamp"].as_str().unwrap_or(""),
                    v["stage"].as_str().unwrap_or(""),
                    v["data"].to_string()
                )
            })
            .unwrap_or_else(|| "No agent events recorded".into());
        self.label(cx, ids!(ev_event_detail), &detail);
    }
    fn frame(&mut self, cx: &mut Cx) -> Res<()> {
        let ci = self.index(cx, ids!(ev_camera));
        let Some(clip) = &mut self.clip else {
            return Ok(());
        };
        if clip.steps.is_empty() {
            self.label(cx, ids!(ev_clock), "No camera recording");
            self.label(cx, ids!(ev_trace), "");
            return Ok(());
        }
        let n = clip.steps.len();
        self.cursor = self.cursor.min(n - 1);
        let tick = clip.steps[self.cursor];
        let sim_seconds = tick as f64 * clip.tick_ns as f64 / 1e9;
        if let Some(camera) = clip.cameras.get(ci) {
            let rgb = clip.rgb(self.cursor, camera).map_err(msg)?;
            VideoPlayerWidgetRefExt::as_video_player(&self.view.widget(cx, ids!(ev_image)))
                .set_frame(
                    cx,
                    rgb.width() as usize,
                    rgb.height() as usize,
                    rgb.as_raw(),
                );
        }
        let unit = if clip.camera_frames > 0 {
            "frame"
        } else {
            "trace sample"
        };
        let phase = clip
            .phases
            .iter()
            .rposition(|p| p["tick"].as_u64().unwrap_or(u64::MAX) <= tick);
        let trace = clip.trace_at(tick).map_err(msg)?;
        if let Some(i) = phase {
            self.view
                .drop_down(cx, ids!(ev_phase))
                .set_selected_item(cx, i);
        }
        let trace=trace.map(|t|{
            let joints=t["joints"].as_array();
            if t["both_feet_loaded"].is_boolean(){
                return format!("Trace · step {tick} · {} joints · both feet loaded {} · room collisions {} · joint excursion {:.5} rad",joints.map_or(0,Vec::len),t["both_feet_loaded"],t["collision_count"],t["max_joint_excursion_rad"].as_f64().unwrap_or_default());
            }
            let shoulder=joints.and_then(|j|j.iter().position(|name|name=="left_j2_pitch")).and_then(|i|Some((t["positions_rad"][i].as_f64()?,t["commanded"][i].as_f64()?)));
            let shoulder=shoulder.map(|(measured,target)|format!(" · left shoulder {measured:.3} / {target:.3} rad measured / commanded")).unwrap_or_default();
            format!("Trace · step {tick} · {} joints · wheels {} · tabletop {} · guard rejections {} · collisions {}{shoulder}",
                joints.map_or(0,Vec::len),t["wheel_supported"],t["tabletop_contact"],t["rejected_motor_writes"],t["collision_count"])
        }).unwrap_or_else(||"No robot trace at this time".into());
        self.label(cx, ids!(ev_trace), &trace);
        self.label(
            cx,
            ids!(ev_clock),
            &format!(
                "{:.2} s   ·   {unit} {} / {n}",
                sim_seconds,
                self.cursor + 1
            ),
        );
        self.view
            .slider(cx, ids!(ev_scrub))
            .set_value(cx, self.cursor as f64 / (n - 1).max(1) as f64);
        Ok(())
    }
    fn operation(&self, op: &str, data: Option<String>) -> Res<String> {
        let e = self.entry()?;
        let root = runtime::root();
        let mut cmd = python_command(&root).map_err(msg)?;
        cmd.arg(root.join("tools/aisle_workbench/trial_revision.py"))
            .arg(op)
            .arg(&e.batch)
            .arg(e.id());
        if let Some(v) = data {
            cmd.arg(v);
        }
        let out = cmd.stdin(Stdio::null()).output().map_err(msg)?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("Trial operation failed")
                .into());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().into())
    }
    fn run(&mut self, cx: &mut Cx, variant: Option<String>) -> Res<()> {
        let path = self.operation(
            if variant.is_some() {
                "edited"
            } else {
                "original"
            },
            variant,
        )?;
        let root = runtime::root();
        let mut cmd: Command = python_command(&root).map_err(msg)?;
        let log = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(Path::new(&path).join("supervisor.log"))
            .map_err(msg)?;
        self.children.push(
            cmd.arg(root.join("tools/aisle_workbench/experiment_job.py"))
                .arg("run")
                .arg(&path)
                .stdin(Stdio::null())
                .stdout(log.try_clone().map_err(msg)?)
                .stderr(log)
                .spawn()
                .map_err(msg)?,
        );
        let job = read(&Path::new(&path).join("batch.json")).map_err(msg)?;
        self.set(cx, ids!(ev_search), "");
        self.load(cx, job["repetitions"][0]["run_id"].as_str())?;
        self.label(cx, ids!(ev_feedback), "Derived run started");
        Ok(())
    }
    fn actions(&mut self, cx: &mut Cx, a: &Actions) -> Res<()> {
        macro_rules! click {
            ($id:ident) => {
                self.view.button(cx, ids!($id)).clicked(a)
            };
        }
        macro_rules! choose {
            ($id:ident) => {
                self.view.drop_down(cx, ids!($id)).selected(a).is_some()
            };
        }
        if click!(ev_refresh) || click!(ev_filter) {
            let id = self.entry().ok().map(|e| e.id().to_string());
            return self.load(cx, id.as_deref());
        }
        if choose!(ev_entries) {
            return self.select(cx, self.index(cx, ids!(ev_entries)));
        }
        if self.current.is_none() {
            return Ok(());
        }
        if click!(ev_options) {
            self.options_open = !self.options_open;
            self.view
                .widget(cx, ids!(ev_options_body))
                .set_visible(cx, self.options_open);
        }
        if choose!(ev_revisions) {
            self.revision(cx)?;
        }
        if choose!(ev_events) {
            self.history(cx);
        }
        if choose!(ev_object) || choose!(ev_axis) {
            self.position(cx);
        }
        if choose!(ev_variants) {
            self.variant_detail(cx);
        }
        let len = self.clip.as_ref().map_or(0, |c| c.steps.len());
        if choose!(ev_camera) {
            self.frame(cx)?;
        }
        if choose!(ev_phase) {
            if let Some(c) = &self.clip {
                if let Some(t) = c
                    .phases
                    .get(self.index(cx, ids!(ev_phase)))
                    .and_then(|p| p["tick"].as_u64())
                {
                    self.cursor = c.index_at(t);
                    self.frame(cx)?;
                }
            }
        }
        if let Some(v) = self.view.slider(cx, ids!(ev_scrub)).slided(a) {
            self.cursor = (v * len.saturating_sub(1) as f64).round() as usize;
            self.frame(cx)?;
        }
        if click!(ev_back) || click!(ev_forward) {
            self.cursor = if click!(ev_back) {
                self.cursor.saturating_sub(1)
            } else {
                (self.cursor + 1).min(len.saturating_sub(1))
            };
            self.frame(cx)?;
        }
        if click!(ev_play) && len > 0 {
            self.playing = if self.playing.is_some() {
                None
            } else {
                if self.cursor + 1 >= len {
                    self.cursor = 0;
                }
                Some(Instant::now())
            };
            self.view.button(cx, ids!(ev_play)).set_text(
                cx,
                if self.playing.is_some() {
                    "Pause"
                } else {
                    "Play"
                },
            );
        }
        if click!(ev_parent) {
            let id = self.entry()?.row["parent_run_id"]
                .as_str()
                .ok_or("No parent")?
                .to_string();
            self.set(cx, ids!(ev_search), "");
            self.load(cx, Some(&id))?;
        }
        if click!(ev_save_notes) {
            let e = self.entry()?;
            let id = e.id().to_string();
            let fields: Value =
                serde_json::from_str(&self.field(cx, ids!(ev_fields))).map_err(msg)?;
            let data = json!({"revision":e.notes["revision"],"name":self.field(cx,ids!(ev_name)),"notes":self.field(cx,ids!(ev_notes)),"tags":self.field(cx,ids!(ev_tags)).split(',').map(str::trim).filter(|s|!s.is_empty()).collect::<Vec<_>>(),"fields":fields});
            self.operation("annotate", Some(data.to_string()))?;
            self.load(cx, Some(&id))?;
            self.label(cx, ids!(ev_feedback), "Notes saved");
        }
        if click!(ev_save_variant) {
            let e = self.entry()?;
            let id = e.id().to_string();
            let item = &self.layout["scene"]["instances"][self.index(cx, ids!(ev_object))];
            let key = item["id"].as_str().ok_or("Choose an object")?;
            let axis = ["x", "y", "z"][self.index(cx, ids!(ev_axis))];
            let pos = self
                .field(cx, ids!(ev_position))
                .parse::<f64>()
                .map_err(|_| "Enter a position in metres")?;
            let raw = self.field(cx, ids!(ev_seed));
            let seeds = raw
                .split([',', ' '])
                .filter(|s| !s.is_empty())
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "Enter comma-separated unsigned seeds")?;
            let log_idx = self.index(cx, ids!(ev_logs));
            let parameters: Value =
                serde_json::from_str(&self.field(cx, ids!(ev_params))).map_err(msg)?;
            let mut data = json!({"name":self.field(cx,ids!(ev_variant_name)),"seeds":seeds,"parameters":parameters,"pose_edits":{},
                    "recording":{"robot_trace":log_idx%2==1,"agent_history":log_idx>=2},"record_cameras":self.index(cx,ids!(ev_capture))==0});
            if self.index(cx, ids!(ev_task_source)) == 1 {
                let (path, sha) = self
                    .prepared
                    .as_ref()
                    .ok_or("Prepare the revised scene and controller in Task first")?;
                data["prepared_controller"] = json!(path);
                data["prepared_sha256"] = json!(sha);
            }
            if item["position_m"][self.index(cx, ids!(ev_axis))].as_f64() != Some(pos) {
                data["pose_edits"][key][axis] = json!(pos);
            }
            self.operation("variant", Some(data.to_string()))?;
            self.load(cx, Some(&id))?;
            self.label(cx, ids!(ev_feedback), "Variant saved");
        }
        if click!(ev_original) {
            self.run(cx, None)?;
        }
        if click!(ev_edited) {
            let id = self
                .variants
                .get(self.index(cx, ids!(ev_variants)))
                .and_then(|v| v["id"].as_str())
                .ok_or("Save a variant first")?
                .into();
            self.run(cx, Some(id))?;
        }
        Ok(())
    }
    fn tick(&mut self, cx: &mut Cx) -> Res<()> {
        if !self.loaded {
            self.loaded = true;
            self.load(cx, None)?;
        }
        self.children
            .retain_mut(|c| c.try_wait().ok().flatten().is_none());
        if let Some(at) = self.playing {
            let elapsed = at.elapsed().as_millis()
                / self.clip.as_ref().map_or(200, |c| c.interval_ms as u128);
            if elapsed > 0 {
                self.cursor += elapsed as usize;
                let n = self.clip.as_ref().map_or(0, |c| c.steps.len());
                self.playing = (self.cursor + 1 < n).then(Instant::now);
                if self.playing.is_none() {
                    self.view.button(cx, ids!(ev_play)).set_text(cx, "Play");
                }
                self.frame(cx)?;
            }
        }
        if self
            .next_poll
            .is_none_or(|t| t.elapsed() > Duration::from_secs(2))
        {
            self.next_poll = Some(Instant::now());
            if let Ok(e) = self.entry() {
                if matches!(e.row["status"].as_str(), Some("planned" | "running")) {
                    let row = file_or_null(&e.trial().join("manifest.json"));
                    if row["status"] != e.row["status"] {
                        let id = e.id().to_string();
                        self.load(cx, Some(&id))?;
                    }
                }
            }
        }
        Ok(())
    }
}
impl TrialEvidenceRef {
    pub fn update(&self, cx: &mut Cx, workbench: &Workbench) {
        if let Some(mut s) = self.borrow_mut() {
            s.prepared = workbench
                .prepared
                .as_ref()
                .zip(workbench.prepared_digest())
                .map(|(p, h)| (p.clone(), h.to_string()));
            if let Err(e) = s.tick(cx) {
                s.label(cx, ids!(ev_feedback), &e);
            }
        }
    }
}
impl Widget for TrialEvidence {
    fn handle_event(&mut self, cx: &mut Cx, e: &Event, scope: &mut Scope) {
        if !self.view.visible {
            return;
        }
        let a = cx.capture_actions(|cx| self.view.handle_event(cx, e, scope));
        if let Err(e) = self.actions(cx, &a) {
            self.label(cx, ids!(ev_feedback), &e);
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}
