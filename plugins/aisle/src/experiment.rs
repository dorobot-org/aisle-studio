use dorobot_studio_core::{api::aisle::Workbench, runtime, survey::task::BoundTask};
use dorobot_studio_plugin::*;
use serde_json::{json, Value};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

mod preview;
mod worker;
use preview::LayoutPreview;
use worker::WorkerSelection;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    mod.widgets.ExperimentPaneBase = #(ExperimentPane::register_widget(vm))
    let Line=View{width:Fill height:Fit flow:Right spacing:5 align:Align{y:0.5}}
    let Text=Label{width:Fill max_lines:4}
    let Field=TextInput{width:Fill height:32}
    let Tool=mod.widgets.ux.ToolButton{}
    mod.widgets.ExperimentPane=set_type_default() do mod.widgets.ExperimentPaneBase{
        width:Fill height:Fill flow:Down spacing:7
        exp_context := Text{}
        Line{exp_configs := DropDown{width:Fill} exp_load := Tool{text:"Open configuration"} exp_save := Tool{text:"Save configuration"} exp_start := mod.widgets.ux.PrimaryButton{text:"Start batch"}}
        View{width:Fill height:Fill flow:Right spacing:12
            View{width:Fill height:Fill flow:Down spacing:6
              Line{exp_history_title := Text{text:"Batch history"} exp_preview := Tool{text:"Preview layout"}}
              exp_batch_history := View{width:Fill height:Fill flow:Down spacing:6
                Line{exp_batches := DropDown{width:Fill} exp_refresh := Tool{text:"Refresh batches"}}
                exp_summary := Text{text:"No batch selected"}
                exp_rows_scroll := ScrollYView{width:Fill height:Fill flow:Down
                    exp_rows := Label{width:Fill max_lines:0 text:"Start a batch to see its repetitions."}}
                Line{exp_prev := Tool{text:"Previous trial"} exp_next := Tool{text:"Next trial"} exp_cancel := Tool{text:"Cancel trial"} exp_retry := Tool{text:"Retry trial"}}
                exp_detail := Text{}
              }
              exp_preview_panel := View{visible:false width:Fill height:Fill flow:Down spacing:5
                exp_preview_status := Text{}
                exp_preview_image := VideoPlayer{width:Fill height:Fill}
                exp_preview_values := Text{}
                Line{exp_preview_prev := Tool{text:"Previous layout"} exp_preview_next := Tool{text:"Next layout"}}
              }
            }
            exp_form := ScrollYView{width:354 height:Fill flow:Down spacing:7
                Label{text:"Run configuration"}
                Line{Label{text:"Name"} exp_name := Field{empty_text:"Experiment name"}}
                Line{Label{text:"Worker"} exp_worker := DropDown{width:Fill} exp_check_worker := Tool{text:"Check worker"}}
                exp_worker_info := Text{}
                Line{Label{text:"Repeats"} exp_count := Field{text:"4"} exp_seed_mode := DropDown{width:175}}
                Line{Label{text:"Seeds"} exp_seed := Field{text:"42" empty_text:"Base seed or comma-separated seeds"}}
                exp_areas := View{visible:false width:Fill height:Fit flow:Down spacing:5
                    Text{text:"Scene start areas"}
                    Line{exp_area := DropDown{width:Fill} exp_area_toggle := Tool{text:"Enable area"}}
                }
                exp_rules := View{visible:false width:Fill height:Fit flow:Down spacing:5
                    Text{text:"Scene experiment ranges"}
                    Line{exp_rule := DropDown{width:Fill} exp_rule_toggle := Tool{text:"Enable range"}}
                    exp_rule_info := Text{}
                }
                Text{text:"Recording channels"}
                exp_scope := DropDown{width:Fill}
                Line{exp_trace := Tool{text:"Robot trace · off"} exp_history := Tool{text:"Agent history · off"}}
                Line{exp_cameras := Tool{text:"Record cameras · on"} exp_inherit := Tool{text:"Use batch defaults"}}
                Text{text:"Custom feature and runtime binding"}
                exp_features := DropDown{width:Fill}
                Line{Label{text:"Name"} exp_feature := Field{empty_text:"Chair spacing"}}
                Line{exp_type := DropDown{width:Fill} exp_units := Field{empty_text:"Units"}}
                exp_binding := DropDown{width:Fill}
                Line{exp_a := DropDown{width:Fill} exp_b := DropDown{width:Fill}}
                Line{exp_axis := DropDown{width:90} exp_default := Field{empty_text:"Default"}}
                Line{exp_low := Field{empty_text:"Minimum"} exp_high := Field{empty_text:"Maximum"}}
                exp_choices := Field{empty_text:"Allowed enum values, comma separated"}
                Line{exp_feature_save := Tool{text:"Apply feature"} exp_feature_delete := Tool{text:"Remove feature"}}
            }
        }
        exp_feedback := Text{}
    }
}
#[derive(Script, ScriptHook, Widget)]
pub struct ExperimentPane {
    #[deref]
    view: View,
    #[rust]
    loaded: bool,
    #[rust]
    controller: Option<PathBuf>,
    #[rust]
    controller_sha: String,
    #[rust]
    task: Option<BoundTask>,
    #[rust]
    config: Value,
    #[rust]
    configs: Vec<PathBuf>,
    #[rust]
    batches: Vec<PathBuf>,
    #[rust]
    batch: Option<PathBuf>,
    #[rust]
    job: Value,
    #[rust]
    selected: usize,
    #[rust]
    last: Option<Instant>,
    #[rust]
    children: Vec<Child>,
    #[rust]
    preview: LayoutPreview,
    #[rust]
    workers: WorkerSelection,
}
fn root() -> PathBuf {
    runtime::user_data().join("aisle")
}
fn json_file(p: &Path) -> io::Result<Value> {
    serde_json::from_slice(&fs::read(p)?).map_err(io::Error::other)
}
fn names(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| s.to_string()).collect()
}
fn placement_labels(placements: &Value) -> Vec<String> {
    let mut values = Vec::new();
    for placement in placements.as_object().into_iter().flatten().map(|(_, v)| v) {
        let pos = &placement["position_m"];
        let coords: Vec<_> = (0..3)
            .filter_map(|i| pos[i].as_f64().map(|n| format!("{n:.3}")))
            .collect();
        let support = placement["support"]["surface"].as_str().unwrap_or("floor");
        values.push(format!(
            "{} on {support} · ({}) m",
            placement["name"].as_str().unwrap_or("Object"),
            coords.join(", ")
        ));
    }
    values
}
const MODES: [&str; 3] = ["sequence", "explicit", "identical"];
const TYPES: [&str; 4] = ["number", "integer", "boolean", "enum"];
const BINDINGS: [&str; 7] = [
    "spacing",
    "position",
    "yaw",
    "friction",
    "ambient",
    "wrist_clearance",
    "visual_profile",
];

impl ExperimentPane {
    fn note(&self, cx: &mut Cx, s: &str) {
        self.view.label(cx, ids!(exp_feedback)).set_text(cx, s);
    }
    fn field(&self, cx: &Cx, id: &[LiveId]) -> String {
        self.view.text_input(cx, id).text().trim().into()
    }
    fn set(&self, cx: &mut Cx, id: &[LiveId], v: &str) {
        self.view.text_input(cx, id).set_text(cx, v);
    }
    fn sel(&self, cx: &Cx, id: &[LiveId]) -> usize {
        self.view.drop_down(cx, id).selected_item()
    }
    fn toggle_scene_option(&mut self, cx: &mut Cx, setting: &str, choice: Value) {
        let mut active = self.config[setting].as_array().cloned().unwrap_or_default();
        if let Some(index) = active.iter().position(|v| v == &choice) {
            active.remove(index);
        } else {
            active.push(choice);
        }
        self.config[setting] = Value::Array(active);
        self.close_preview(cx);
    }
    fn initialize(&mut self, cx: &mut Cx) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Err(error) = self.load_workers(cx) {
            self.note(cx, &error);
        }
        self.config = json!({"name":"Nano experiment","repeat_count":4,"base_seed":42,"seed_mode":"sequence","parameters":[],"recording":{"robot_trace":false,"agent_history":false},"overrides":{},"record_cameras":true});
        for (id, labels) in [
            (
                ids!(exp_seed_mode) as &[LiveId],
                names(&["Sequential seeds", "Explicit seed list", "Identical inputs"]),
            ),
            (ids!(exp_type), names(&TYPES)),
            (
                ids!(exp_binding),
                names(&[
                    "Pair spacing",
                    "Object position",
                    "Object yaw",
                    "Object friction",
                    "Ambient light",
                    "Wrist clearance",
                    "Camera profile",
                ]),
            ),
            (ids!(exp_axis), names(&["X", "Y", "Z"])),
        ] {
            self.view.drop_down(cx, id).set_labels(cx, labels);
        }
        self.form(cx);
        if let Err(e) = self.archives(cx) {
            self.note(cx, &e.to_string());
        } else {
            for folder in self.batches.clone() {
                if let Err(error) = self.resume_unfinished(&folder) {
                    self.note(cx, &error);
                }
            }
        }
    }
    fn resume_unfinished(&mut self, folder: &Path) -> Result<(), String> {
        let job = json_file(&folder.join("batch.json")).map_err(|e| e.to_string())?;
        let rows = job["repetitions"]
            .as_array()
            .ok_or("Missing batch repetitions")?;
        let pending = rows
            .iter()
            .any(|r| matches!(r["status"].as_str(), Some("planned" | "running")));
        let retry_pending = fs::read_dir(folder.join("requests"))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "retry"))
            .any(|p| {
                !rows
                    .iter()
                    .any(|row| row["run_id"].as_str() == p.file_stem().and_then(|id| id.to_str()))
            });
        if pending || retry_pending {
            self.supervise(folder)?;
        }
        Ok(())
    }
    fn context(&mut self, cx: &mut Cx, w: &Workbench) {
        let opening = !self.loaded;
        self.initialize(cx);
        if self.controller != w.prepared
            || self.controller_sha != w.prepared_digest().unwrap_or_default()
        {
            self.close_preview(cx);
            self.controller = w.prepared.clone();
            self.controller_sha = w.prepared_digest().unwrap_or_default().to_string();
            self.task = self
                .controller
                .as_ref()
                .and_then(|p| fs::read(p.with_file_name("bound-task.json")).ok())
                .and_then(|d| serde_json::from_slice(&d).ok());
            if let Some(config) = self.config.as_object_mut() {
                config.remove("active_start_areas");
                config.remove("active_scene_rules");
            }
            self.area_choices(cx);
            self.rule_choices(cx);
            let rows = self
                .task
                .as_ref()
                .map(|t| {
                    t.scene
                        .instances
                        .iter()
                        .map(|i| i.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for id in [ids!(exp_a) as &[LiveId], ids!(exp_b)] {
                self.view.drop_down(cx, id).set_labels(cx, rows.clone());
            }
        }
        if opening && self.task.as_ref().is_some_and(|t| t.robot == "G1") {
            self.config["name"] = json!("G1 standing experiment");
            self.form(cx);
        }
        if opening && !self.controller_sha.is_empty() {
            if let Some((i, cfg)) = self
                .configs
                .iter()
                .enumerate()
                .rev()
                .filter_map(|(i, p)| json_file(p).ok().map(|v| (i, v)))
                .find(|(_, v)| {
                    v["controller_sha256"].as_str() == Some(self.controller_sha.as_str())
                })
            {
                self.config = cfg;
                self.form(cx);
                self.view
                    .drop_down(cx, ids!(exp_configs))
                    .set_selected_item(cx, i);
            }
        }
        self.update_worker(cx);
        let text = self
            .task
            .as_ref()
            .map(|t| {
                format!(
                    "{} · scene v{}   /   {}   /   {}   /   controller {}   /   {}",
                    t.scene.name,
                    t.scene.revision,
                    t.robot,
                    t.instruction,
                    self.controller_sha.get(..10).unwrap_or(""),
                    self.worker_alias()
                )
            })
            .unwrap_or_else(|| "Prepare a scene-bound controller in Task".into());
        self.view.label(cx, ids!(exp_context)).set_text(cx, &text);
        self.view.button(cx, ids!(exp_start)).set_enabled(
            cx,
            self.task.is_some() && self.workers.ready && self.workers.start.is_none(),
        );
        self.view
            .button(cx, ids!(exp_preview))
            .set_enabled(cx, self.task.is_some());
        self.poll_preview(cx);
        self.children
            .retain_mut(|p| p.try_wait().ok().flatten().is_none());
        if self
            .last
            .is_none_or(|v| v.elapsed() > Duration::from_millis(500))
        {
            self.last = Some(Instant::now());
            self.poll(cx);
        }
    }
    fn form(&self, cx: &mut Cx) {
        let cfg = &self.config;
        self.show_worker_choice(cx);
        self.set(cx, ids!(exp_name), cfg["name"].as_str().unwrap_or(""));
        self.set(cx, ids!(exp_count), &cfg["repeat_count"].to_string());
        let mode = MODES
            .iter()
            .position(|m| Some(*m) == cfg["seed_mode"].as_str())
            .unwrap_or(0);
        self.view
            .drop_down(cx, ids!(exp_seed_mode))
            .set_selected_item(cx, mode);
        let seed = if mode == 1 {
            cfg["seeds"]
                .as_array()
                .into_iter()
                .flatten()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            cfg["base_seed"].to_string()
        };
        self.set(cx, ids!(exp_seed), &seed);
        self.scopes(cx);
        self.channels(cx);
        self.feature_names(cx);
        self.area_choices(cx);
        self.rule_choices(cx);
    }
    fn rule_choices(&self, cx: &mut Cx) {
        let rules = self
            .task
            .as_ref()
            .map(|t| t.scene.rules.as_slice())
            .unwrap_or_default();
        self.view
            .widget(cx, ids!(exp_rules))
            .set_visible(cx, !rules.is_empty());
        let choices = self.view.drop_down(cx, ids!(exp_rule));
        choices.set_labels(cx, rules.iter().map(|r| r.name.clone()).collect());
        if choices.selected_item() >= rules.len() {
            choices.set_selected_item(cx, 0);
        }
        self.rule_button(cx);
    }
    fn rule_button(&self, cx: &mut Cx) {
        let Some(task) = &self.task else {
            return;
        };
        let Some(rule) = task.scene.rules.get(self.sel(cx, ids!(exp_rule))) else {
            return;
        };
        let enabled = self.config["active_scene_rules"]
            .as_array()
            .is_some_and(|rules| rules.iter().any(|n| n.as_str() == Some(&rule.name)));
        self.view.button(cx, ids!(exp_rule_toggle)).set_text(
            cx,
            if enabled {
                "Disable range"
            } else {
                "Enable range"
            },
        );
        let target = rule
            .instance
            .as_ref()
            .and_then(|id| task.scene.instances.iter().find(|i| &i.id == id))
            .map_or("Room", |i| &i.name);
        self.view.label(cx, ids!(exp_rule_info)).set_text(
            cx,
            &format!(
                "{} · {} · {} to {}",
                target,
                rule.property.label(),
                rule.range[0],
                rule.range[1]
            ),
        );
    }
    fn area_instances(
        &self,
    ) -> impl Iterator<Item = &dorobot_studio_core::survey::scene::Instance> {
        self.task
            .iter()
            .flat_map(|t| t.scene.instances.iter())
            .filter(|i| i.start_area.is_some())
    }
    fn area_choices(&self, cx: &mut Cx) {
        let names = self
            .area_instances()
            .map(|i| i.name.clone())
            .collect::<Vec<_>>();
        self.view
            .widget(cx, ids!(exp_areas))
            .set_visible(cx, !names.is_empty());
        self.view
            .drop_down(cx, ids!(exp_area))
            .set_labels(cx, names);
        self.area_button(cx);
    }
    fn area_button(&self, cx: &mut Cx) {
        let choice = self.area_instances().nth(self.sel(cx, ids!(exp_area)));
        let active = choice.is_some_and(|i| {
            self.config["active_start_areas"]
                .as_array()
                .is_some_and(|v| v.iter().any(|id| id.as_str() == Some(&i.id)))
        });
        self.view.button(cx, ids!(exp_area_toggle)).set_text(
            cx,
            if active {
                "Disable area"
            } else {
                "Enable area"
            },
        );
    }
    fn scopes(&self, cx: &mut Cx) {
        let count = self
            .field(cx, ids!(exp_count))
            .parse::<usize>()
            .unwrap_or(0)
            .min(10000);
        self.view.drop_down(cx, ids!(exp_scope)).set_labels(
            cx,
            std::iter::once("Batch defaults".into())
                .chain((1..=count).map(|i| format!("Repetition {i}")))
                .collect(),
        );
    }
    fn channels(&self, cx: &mut Cx) {
        let i = self.sel(cx, ids!(exp_scope)).to_string();
        for (key, id, label) in [
            ("robot_trace", ids!(exp_trace) as &[LiveId], "Robot trace"),
            ("agent_history", ids!(exp_history), "Agent history"),
        ] {
            let v = self.config["overrides"][&i][key]
                .as_bool()
                .or_else(|| self.config["recording"][key].as_bool())
                .unwrap_or(false);
            self.view
                .button(cx, id)
                .set_text(cx, &format!("{label} · {}", if v { "on" } else { "off" }));
        }
        self.view.button(cx, ids!(exp_cameras)).set_text(
            cx,
            if self.config["record_cameras"] == false {
                "Record cameras · off"
            } else {
                "Record cameras · on"
            },
        );
    }
    fn feature_names(&self, cx: &mut Cx) {
        let items = std::iter::once("New feature".to_string())
            .chain(
                self.config["parameters"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v["name"].as_str().map(String::from)),
            )
            .collect();
        self.view
            .drop_down(cx, ids!(exp_features))
            .set_labels(cx, items);
    }
    fn config_input(&self, cx: &Cx) -> Result<Value, String> {
        let mut cfg = self.config.clone();
        let name = self.field(cx, ids!(exp_name));
        if name.is_empty() {
            return Err("Name the experiment".into());
        }
        cfg["name"] = name.into();
        let count = self
            .field(cx, ids!(exp_count))
            .parse::<u64>()
            .ok()
            .filter(|n| (1..=10000).contains(n))
            .ok_or("Enter 1–10000 repetitions")?;
        cfg["repeat_count"] = count.into();
        let mode = MODES[self.sel(cx, ids!(exp_seed_mode))];
        cfg["seed_mode"] = mode.into();
        let raw = self.field(cx, ids!(exp_seed));
        if mode == "explicit" {
            let seeds = raw
                .split([',', ' '])
                .filter(|v| !v.is_empty())
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "Enter unsigned 32-bit seeds")?;
            if seeds.len() != count as usize {
                return Err("Enter one seed per repetition".into());
            }
            cfg["seeds"] = json!(seeds);
        } else {
            cfg["base_seed"] = raw
                .parse::<u32>()
                .map_err(|_| "Enter an unsigned 32-bit base seed")?
                .into();
        }
        Ok(cfg)
    }
    fn archives(&mut self, cx: &mut Cx) -> io::Result<()> {
        let store = root();
        let files = |path: PathBuf| -> io::Result<Vec<PathBuf>> {
            if !path.exists() {
                return Ok(vec![]);
            }
            let mut list = fs::read_dir(path)?
                .map(|r| r.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()?;
            list.sort();
            Ok(list)
        };
        self.configs = files(store.join("experiment-configs"))?
            .into_iter()
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        self.view.drop_down(cx, ids!(exp_configs)).set_labels(
            cx,
            self.configs
                .iter()
                .map(|p| {
                    json_file(p)
                        .ok()
                        .and_then(|v| v["name"].as_str().map(String::from))
                        .unwrap_or_else(|| "Unavailable configuration".into())
                })
                .collect(),
        );
        self.batches = files(store.join("experiments"))?
            .into_iter()
            .filter(|p| p.join("batch.json").is_file())
            .collect();
        self.batches.sort_by_cached_key(|p| {
            json_file(&p.join("batch.json"))
                .ok()
                .and_then(|v| v["created"].as_str().map(String::from))
                .unwrap_or_default()
        });
        self.view.drop_down(cx, ids!(exp_batches)).set_labels(
            cx,
            self.batches
                .iter()
                .map(|p| {
                    json_file(&p.join("batch.json"))
                        .ok()
                        .map(|v| {
                            format!(
                                "{} · {}",
                                v["configuration"]["name"].as_str().unwrap_or("Batch"),
                                v["created"].as_str().unwrap_or("").get(..19).unwrap_or("")
                            )
                        })
                        .unwrap_or_else(|| "Unavailable batch".into())
                })
                .collect(),
        );
        if self.batch.is_none() {
            self.batch = self.batches.last().cloned();
        }
        if let Some(i) = self
            .batches
            .iter()
            .position(|p| Some(p) == self.batch.as_ref())
        {
            self.view
                .drop_down(cx, ids!(exp_batches))
                .set_selected_item(cx, i);
        }
        self.poll(cx);
        Ok(())
    }
    fn poll(&mut self, cx: &mut Cx) {
        let Some(p) = &self.batch else { return };
        match json_file(&p.join("batch.json")) {
            Ok(v) if v != self.job => {
                self.job = v;
                self.rows(cx);
            }
            Err(e) => self.note(cx, &e.to_string()),
            _ => {}
        }
    }
    fn rows(&mut self, cx: &mut Cx) {
        let Some(rows) = self.job["repetitions"].as_array() else {
            return;
        };
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        let Some(selected) = rows.get(self.selected) else {
            return;
        };
        let s = |r: &Value, key: &str| {
            if r["recording"][key] == true {
                "on"
            } else {
                "off"
            }
        };
        self.view.label(cx, ids!(exp_summary)).set_text(
            cx,
            &format!(
                "{}   ·   {} repetitions",
                self.job["configuration"]["name"].as_str().unwrap_or(""),
                rows.len()
            ),
        );
        let lines = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                format!(
                    "{}  {:>2}     Seed {}     {}     Trace {}    History {}",
                    if i == self.selected { "›" } else { " " },
                    r["repetition"],
                    r["seed"],
                    r["status"].as_str().unwrap_or(""),
                    s(r, "robot_trace"),
                    s(r, "agent_history")
                )
            })
            .collect::<Vec<_>>();
        self.view
            .label(cx, ids!(exp_rows))
            .set_text(cx, &lines.join("\n"));
        let sampled = selected["sampled_values"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| {
                let value = v["value"]
                    .as_f64()
                    .map(|n| format!("{n:.4}"))
                    .unwrap_or_else(|| v["value"].to_string());
                format!("{k} {value} {}", v["unit"].as_str().unwrap_or(""))
            })
            .chain(placement_labels(&selected["placements"]))
            .collect::<Vec<_>>()
            .join(" · ");
        self.view.label(cx, ids!(exp_detail)).set_text(
            cx,
            &format!(
                "Trial {} · {} · {}\n{sampled}\n{}",
                selected["run_id"].as_str().unwrap_or(""),
                selected["status"].as_str().unwrap_or(""),
                self.job["configuration"]["worker"]
                    .as_str()
                    .unwrap_or("local"),
                selected["reason"].as_str().unwrap_or("")
            ),
        );
        let active = matches!(selected["status"].as_str(), Some("planned" | "running"));
        self.view
            .button(cx, ids!(exp_cancel))
            .set_enabled(cx, active);
        self.view
            .button(cx, ids!(exp_retry))
            .set_enabled(cx, !active);
    }
    fn load_feature(&self, cx: &mut Cx, index: usize) {
        let p = index
            .checked_sub(1)
            .and_then(|i| self.config["parameters"].get(i))
            .unwrap_or(&Value::Null);
        let scalar = |v: &Value| {
            v.as_str().map(String::from).unwrap_or_else(|| {
                if v.is_null() {
                    String::new()
                } else {
                    v.to_string()
                }
            })
        };
        for (id, text) in [
            (ids!(exp_feature) as &[LiveId], scalar(&p["name"])),
            (ids!(exp_units), scalar(&p["unit"])),
            (ids!(exp_default), scalar(&p["default"])),
            (ids!(exp_low), scalar(&p["range"][0])),
            (ids!(exp_high), scalar(&p["range"][1])),
            (
                ids!(exp_choices),
                p["allowed_values"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ] {
            self.set(cx, id, &text);
        }
        let binding = &p["binding"];
        for (id, i) in [
            (
                ids!(exp_type) as &[LiveId],
                TYPES.iter().position(|v| Some(*v) == p["type"].as_str()),
            ),
            (
                ids!(exp_binding),
                BINDINGS
                    .iter()
                    .position(|v| Some(*v) == binding["kind"].as_str()),
            ),
            (
                ids!(exp_axis),
                ["x", "y", "z"]
                    .iter()
                    .position(|v| Some(*v) == binding["axis"].as_str()),
            ),
        ] {
            self.view
                .drop_down(cx, id)
                .set_selected_item(cx, i.unwrap_or(0));
        }
        if let Some(t) = &self.task {
            for (id, value) in [
                (
                    ids!(exp_a) as &[LiveId],
                    binding["instance"]
                        .as_str()
                        .or(binding["instances"][0].as_str()),
                ),
                (ids!(exp_b), binding["instances"][1].as_str()),
            ] {
                let i = t
                    .scene
                    .instances
                    .iter()
                    .position(|obj| Some(obj.id.as_str()) == value)
                    .unwrap_or(0);
                self.view.drop_down(cx, id).set_selected_item(cx, i);
            }
        }
    }
    fn feature(&self, cx: &Cx) -> Result<Value, String> {
        let name = self.field(cx, ids!(exp_feature));
        if name.is_empty() {
            return Err("Name the custom feature".into());
        }
        let kind = TYPES[self.sel(cx, ids!(exp_type))];
        let convert = |text: String| -> Result<Value, String> {
            match kind {
                "number" => text
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .map(Value::from)
                    .ok_or("Enter a finite number".into()),
                "integer" => text
                    .parse::<i64>()
                    .map(Value::from)
                    .map_err(|_| "Enter an integer".into()),
                "boolean" => text
                    .parse::<bool>()
                    .map(Value::from)
                    .map_err(|_| "Use true or false".into()),
                _ => Ok(text.into()),
            }
        };
        let binding = BINDINGS[self.sel(cx, ids!(exp_binding))];
        let target = |id: &[LiveId]| {
            self.task
                .as_ref()
                .and_then(|t| t.scene.instances.get(self.sel(cx, id)))
                .map(|o| o.id.clone())
                .ok_or("Choose a scene instance")
        };
        let axis = ["x", "y", "z"][self.sel(cx, ids!(exp_axis))];
        let binds = match binding {
            "spacing" => {
                json!({"kind":binding,"axis":axis,"instances":[target(ids!(exp_a))?,target(ids!(exp_b))?]})
            }
            "position" | "yaw" | "friction" => {
                json!({"kind":binding,"axis":axis,"instance":target(ids!(exp_a))?})
            }
            _ => json!({"kind":binding}),
        };
        let mut p = json!({"name":name,"type":kind,"unit":self.field(cx,ids!(exp_units)),"default":convert(self.field(cx,ids!(exp_default)))?,"binding":binds});
        let low = self.field(cx, ids!(exp_low));
        let high = self.field(cx, ids!(exp_high));
        if matches!(kind, "number" | "integer") && (!low.is_empty() || !high.is_empty()) {
            p["range"] = json!([convert(low)?, convert(high)?]);
        }
        if kind == "enum" {
            p["allowed_values"] = json!(self
                .field(cx, ids!(exp_choices))
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>());
        }
        Ok(p)
    }
    fn save_config(&mut self, cx: &mut Cx) -> Result<PathBuf, String> {
        self.config = self.config_input(cx)?;
        if self.worker_alias() != "local" {
            if !self.workers.ready {
                return Err("Check the selected worker before saving this configuration".into());
            }
            self.config["worker_id"] = self.workers.identity.clone().into();
        }
        let mut cfg = self.config.clone();
        if let Some(task) = &self.task {
            cfg["scene"] = json!({"id":task.scene.id,"revision":task.scene.revision});
        }
        cfg["controller_sha256"] = self.controller_sha.clone().into();
        if let Some((index, path)) = self
            .configs
            .iter()
            .enumerate()
            .find(|(_, path)| json_file(path).is_ok_and(|old| old == cfg))
        {
            self.view
                .drop_down(cx, ids!(exp_configs))
                .set_selected_item(cx, index);
            return Ok(path.clone());
        }
        let dir = root().join("experiment-configs");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let tick = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let p = dir.join(format!("{tick}.json"));
        fs::write(
            &p,
            serde_json::to_vec_pretty(&cfg).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        self.archives(cx).map_err(|e| e.to_string())?;
        self.view
            .drop_down(cx, ids!(exp_configs))
            .set_selected_item(cx, self.configs.len().saturating_sub(1));
        Ok(p)
    }
    fn command(&self) -> Result<Command, String> {
        let repo = runtime::root();
        let mut cmd =
            dorobot_studio_core::api::aisle::python_command(&repo).map_err(|e| e.to_string())?;
        cmd.arg(repo.join("tools/aisle_workbench/experiment_job.py"))
            .stdin(Stdio::null());
        Ok(cmd)
    }
    fn output(&self, cmd: &mut Command) -> Result<String, String> {
        let p = cmd.output().map_err(|e| e.to_string())?;
        if !p.status.success() {
            let msg = String::from_utf8_lossy(&p.stderr);
            return Err(msg
                .lines()
                .last()
                .unwrap_or("Experiment command failed")
                .into());
        }
        Ok(String::from_utf8_lossy(&p.stdout).trim().into())
    }
    fn supervise(&mut self, folder: &Path) -> Result<(), String> {
        let log = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(folder.join("supervisor.log"))
            .map_err(|e| e.to_string())?;
        self.children.push(
            self.command()?
                .arg("run")
                .arg(folder)
                .stdout(log.try_clone().map_err(|e| e.to_string())?)
                .stderr(log)
                .spawn()
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    fn actions(&mut self, cx: &mut Cx, a: &Actions) -> Result<(), String> {
        macro_rules! clicked {
            ($id:expr) => {
                self.view.button(cx, $id).clicked(a)
            };
        }
        if let Some(i) = self.view.drop_down(cx, ids!(exp_worker)).selected(a) {
            if let Some(alias) = self.workers.aliases.get(i) {
                self.config["worker"] = alias.clone().into();
                self.config.as_object_mut().unwrap().remove("worker_id");
                self.workers.key = None;
                self.update_worker(cx);
            }
        }
        if clicked!(ids!(exp_check_worker)) {
            self.load_workers(cx)?;
            self.show_worker_choice(cx);
            self.workers.key = None;
            self.update_worker(cx);
            return Ok(());
        }
        if clicked!(ids!(exp_preview)) {
            if self.preview.repetition == 0 {
                self.request_preview(cx, 1)?;
            } else {
                self.close_preview(cx);
            }
            return Ok(());
        }
        if self
            .view
            .drop_down(cx, ids!(exp_area))
            .selected(a)
            .is_some()
        {
            self.area_button(cx);
        }
        if self
            .view
            .drop_down(cx, ids!(exp_rule))
            .selected(a)
            .is_some()
        {
            self.rule_button(cx);
        }
        if clicked!(ids!(exp_rule_toggle)) {
            let rule = self
                .task
                .as_ref()
                .and_then(|t| t.scene.rules.get(self.sel(cx, ids!(exp_rule))))
                .ok_or("Select a scene range")?;
            let key = json!(rule.name);
            self.toggle_scene_option(cx, "active_scene_rules", key);
            self.rule_button(cx);
            return Ok(());
        }
        if clicked!(ids!(exp_area_toggle)) {
            let id = self
                .area_instances()
                .nth(self.sel(cx, ids!(exp_area)))
                .map(|i| json!(i.id))
                .ok_or("Select a scene start area")?;
            self.toggle_scene_option(cx, "active_start_areas", id);
            self.area_button(cx);
            return Ok(());
        }
        if clicked!(ids!(exp_preview_prev)) || clicked!(ids!(exp_preview_next)) {
            let i = self.preview.repetition;
            let next = if clicked!(ids!(exp_preview_next)) {
                i + 1
            } else {
                i.saturating_sub(1)
            };
            self.request_preview(cx, next)?;
            return Ok(());
        }
        if self.preview.repetition > 0
            && (self
                .view
                .text_input(cx, ids!(exp_seed))
                .changed(a)
                .is_some()
                || self
                    .view
                    .text_input(cx, ids!(exp_count))
                    .changed(a)
                    .is_some()
                || self
                    .view
                    .drop_down(cx, ids!(exp_seed_mode))
                    .selected(a)
                    .is_some()
                || clicked!(ids!(exp_feature_save))
                || clicked!(ids!(exp_feature_delete))
                || clicked!(ids!(exp_load))
                || clicked!(ids!(exp_start)))
        {
            self.close_preview(cx);
        }
        if clicked!(ids!(exp_refresh)) {
            self.archives(cx).map_err(|e| e.to_string())?;
            return Ok(());
        }
        if self
            .view
            .text_input(cx, ids!(exp_count))
            .changed(a)
            .is_some()
        {
            self.scopes(cx);
        }
        if self
            .view
            .drop_down(cx, ids!(exp_scope))
            .selected(a)
            .is_some()
        {
            self.channels(cx);
        }
        if let Some(i) = self.view.drop_down(cx, ids!(exp_features)).selected(a) {
            self.load_feature(cx, i);
        }
        if let Some(i) = self.view.drop_down(cx, ids!(exp_batches)).selected(a) {
            self.batch = self.batches.get(i).cloned();
            self.selected = 0;
            self.job = Value::Null;
            self.poll(cx);
        }
        if clicked!(ids!(exp_feature_save)) || clicked!(ids!(exp_feature_delete)) {
            let i = self.sel(cx, ids!(exp_features));
            let save = clicked!(ids!(exp_feature_save));
            let p = if save { Some(self.feature(cx)?) } else { None };
            let items = self.config["parameters"]
                .as_array_mut()
                .ok_or("Invalid feature list")?;
            let index = if let Some(p) = p {
                if i == 0 {
                    if items.iter().any(|x| x["name"] == p["name"]) {
                        return Err("That feature name already exists".into());
                    }
                    items.push(p);
                    items.len()
                } else {
                    items[i - 1] = p;
                    i
                }
            } else {
                if i > 0 {
                    items.remove(i - 1);
                }
                0
            };
            self.feature_names(cx);
            self.view
                .drop_down(cx, ids!(exp_features))
                .set_selected_item(cx, index);
            self.load_feature(cx, index);
            self.note(cx, "Updated runtime features");
            return Ok(());
        }
        if clicked!(ids!(exp_cameras)) {
            self.config["record_cameras"] = json!(self.config["record_cameras"] != true);
            self.channels(cx);
            return Ok(());
        }
        for (button, key) in [
            (ids!(exp_trace) as &[LiveId], "robot_trace"),
            (ids!(exp_history), "agent_history"),
        ] {
            if clicked!(button) {
                let i = self.sel(cx, ids!(exp_scope));
                let default = self.config["recording"][key].as_bool().unwrap_or(false);
                if i == 0 {
                    self.config["recording"][key] = (!default).into();
                } else {
                    let j = i.to_string();
                    let on = self.config["overrides"][&j][key]
                        .as_bool()
                        .unwrap_or(default);
                    self.config["overrides"][&j][key] = (!on).into();
                }
                self.channels(cx);
                return Ok(());
            }
        }
        if clicked!(ids!(exp_inherit)) {
            let i = self.sel(cx, ids!(exp_scope)).to_string();
            if let Some(v) = self.config["overrides"].as_object_mut() {
                v.remove(&i);
            }
            self.channels(cx);
            return Ok(());
        }
        if clicked!(ids!(exp_load)) {
            let p = self
                .configs
                .get(self.sel(cx, ids!(exp_configs)))
                .ok_or("Save a configuration first")?;
            let cfg = json_file(p).map_err(|e| e.to_string())?;
            if cfg["controller_sha256"].as_str() != Some(&self.controller_sha) {
                return Err("Select the controller pinned by this configuration in Task".into());
            }
            self.config = cfg;
            self.form(cx);
            self.note(cx, "Configuration opened");
            return Ok(());
        }
        if clicked!(ids!(exp_save)) {
            self.save_config(cx)?;
            self.note(cx, "Configuration saved");
            return Ok(());
        }
        if clicked!(ids!(exp_start)) {
            let config = self.save_config(cx)?;
            let controller = self
                .controller
                .as_ref()
                .ok_or("Prepare a scene-bound controller")?;
            let mut command = self.command()?;
            command
                .arg("create")
                .arg(root().join("experiments"))
                .arg(config)
                .arg(controller)
                .arg("--controller-sha256")
                .arg(&self.controller_sha);
            self.workers.start = Some(worker::background(command));
            self.note(cx, "Preparing batch");
            return Ok(());
        }
        if clicked!(ids!(exp_prev)) || clicked!(ids!(exp_next)) {
            let n = self.job["repetitions"].as_array().map_or(0, Vec::len);
            if n > 0 {
                self.selected = if clicked!(ids!(exp_prev)) {
                    (self.selected + n - 1) % n
                } else {
                    (self.selected + 1) % n
                };
                self.rows(cx);
            }
        } else if clicked!(ids!(exp_cancel)) || clicked!(ids!(exp_retry)) {
            let retry = clicked!(ids!(exp_retry));
            let folder = self.batch.clone().ok_or("Select a batch")?;
            let id = self.job["repetitions"][self.selected]["run_id"]
                .as_str()
                .ok_or("Select a trial")?;
            self.output(
                self.command()?
                    .arg(if retry { "retry" } else { "cancel" })
                    .arg(&folder)
                    .arg(id),
            )?;
            self.supervise(&folder)?;
            self.note(
                cx,
                if retry {
                    "Retry requested"
                } else {
                    "Cancellation requested"
                },
            );
        }
        Ok(())
    }
}
impl ExperimentPaneRef {
    pub fn update(&self, cx: &mut Cx, w: &Workbench) {
        if let Some(mut p) = self.borrow_mut() {
            p.context(cx, w);
        }
    }
}
impl Widget for ExperimentPane {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if !self.view.visible {
            return;
        }
        let actions = cx.capture_actions(|cx| self.view.handle_event(cx, event, scope));
        if let Err(e) = self.actions(cx, &actions) {
            self.note(cx, &e);
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}
