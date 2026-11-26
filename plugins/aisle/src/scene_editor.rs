use dorobot_studio_core::{
    runtime,
    survey::{
        asset::Asset,
        scene::{
            validation::Support,
            viewport::{Camera, Viewport, HEIGHT, WIDTH},
            Revision, Scene, SceneEditor, StartArea,
        },
        Survey,
    },
    widgets::video_player::VideoPlayerWidgetRefExt,
};
use dorobot_studio_plugin::*;
use std::collections::HashMap;
mod agents;
mod rules;
use dorobot_studio_core::survey::scene::agent_edit::{changed_ids, EditSource};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    mod.widgets.SceneEditorPaneBase = #(SceneEditorPane::register_widget(vm))
    let Tool = mod.widgets.ux.ToolButton{}
    let Field = TextInput{width:Fill height:32}
    let Row = View{width:Fill height:Fit flow:Right spacing:6 align:Align{y:0.5}}
    let Caption = Label{width:Fill max_lines:3}
    mod.widgets.SceneEditorPane = set_type_default() do mod.widgets.SceneEditorPaneBase{
        width:Fill height:Fill flow:Down spacing:7
        Row{library := DropDown{width:210} open_scene := Tool{text:"Open"}
            new_scene := Tool{text:"New"} scene_name := Field{empty_text:"Scene name"}
            save := mod.widgets.ux.PrimaryButton{text:"Save revision"} save_as := Tool{text:"Save As"}}
        header := Caption{}
        View{width:Fill height:Fill flow:Right spacing:8
            View{width:Fill height:Fill flow:Down spacing:6
                Row{top_view := Tool{text:"Top view"} undo := Tool{text:"Undo"} redo := Tool{text:"Redo"} viewpoint := Caption{}}
                canvas := VideoPlayer{width:Fill height:Fill}
                Row{proposal_provider := DropDown{width:158 labels:["Local commands" "Codex" "DeepSeek via Octos"]} instruction := Field{empty_text:"Describe a room edit"} propose := Tool{text:"Preview"}}
                reference_row := Row{reference_prompt := Caption{} reference_choices := DropDown{width:150} confirm_reference := Tool{text:"Use selection"}}
                source_label := Caption{}
                proposal_row := Row{summary := Caption{} apply := Tool{text:"Apply proposal"} cancel := Tool{text:"Discard proposal"}}
            }
            inspector := ScrollYView{width:283 height:Fill flow:Down spacing:6
                Caption{text:"Room size in metres"}
                Row{Label{text:"W"} room_w := Field{} Label{text:"D"} room_d := Field{} apply_room := Tool{text:"Set"}}
                Caption{text:"Asset palette"}
                Row{palette := DropDown{width:Fill} reload := Tool{text:"Refresh"}}
                palette_tile := View{width:Fill height:36 align:Align{x:0.5 y:0.5} show_bg:true draw_bg.color:#d5e4ec Label{text:"Drag selected asset into room"}}
                add := Tool{text:"Place at centre"}
                Caption{text:"Scene objects"}
                objects := DropDown{width:Fill}
                selected_name := Field{empty_text:"Instance name"}
                Row{Label{text:"X"} x := Field{} Label{text:"Y"} y := Field{}}
                Row{Label{text:"Z"} z := Field{} Label{text:"Yaw °"} yaw := Field{}}
                Row{update := Tool{text:"Apply pose"} duplicate := Tool{text:"Duplicate"} remove := Tool{text:"Delete"}}
                pin := Caption{}
                Row{support := DropDown{width:Fill} snap := Tool{text:"Snap"}}
                Row{validation := Caption{} next_issue := Tool{text:"Inspect issue"}}
                discard := Tool{text:"Reopen saved layout"}
                Caption{text:"Randomized start area"}
                area_support := DropDown{width:Fill}
                Caption{text:"Bounds from support centre in metres. Leave blank for the whole surface."}
                Row{area_x_min := Field{empty_text:"Min X"} area_x_max := Field{empty_text:"Max X"}}
                Row{area_y_min := Field{empty_text:"Min Y"} area_y_max := Field{empty_text:"Max Y"}}
                Row{area_save := Tool{text:"Set start area"} area_clear := Tool{text:"Remove area"}}
                area_status := Caption{}
                Caption{text:"Experiment ranges"}
                rule_list := DropDown{width:Fill}
                Row{Label{text:"Name"} rule_name := Field{empty_text:"Range name"}}
                Row{Label{text:"Property"} rule_property := DropDown{width:Fill}}
                rule_target_row := Row{Label{text:"Object"} rule_target := DropDown{width:Fill}}
                Row{Label{text:"Min"} rule_min := Field{} Label{text:"Max"} rule_max := Field{}}
                Row{rule_apply := Tool{text:"Save range"} rule_delete := Tool{text:"Remove range"}}
            }
        }
        feedback := Caption{}
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct SceneEditorPane {
    #[deref]
    view: View,
    #[rust]
    editor: SceneEditor,
    #[rust]
    loaded: bool,
    #[rust]
    baseline: Scene,
    #[rust]
    catalog: Vec<Asset>,
    #[rust]
    pinned: HashMap<Revision, Asset>,
    #[rust]
    saved: Vec<Scene>,
    #[rust]
    selected: Option<String>,
    #[rust]
    rule_selected: Option<String>,
    #[rust]
    top: bool,
    #[rust]
    frame: Option<Viewport>,
    #[rust]
    candidate: Option<Scene>,
    #[rust]
    review: agents::Review,
    #[rust]
    drag: Option<Drag>,
    #[rust]
    dragging_asset: bool,
}

struct Drag {
    before: Scene,
    id: String,
    offset: [f64; 2],
}
fn project() -> Survey {
    Survey::new(runtime::user_data().join("aisle").join("survey"))
}

impl SceneEditorPane {
    fn note(&self, cx: &mut Cx, text: &str) {
        self.view.label(cx, ids!(feedback)).set_text(cx, text);
    }
    fn text(&self, cx: &Cx, id: &[LiveId]) -> String {
        self.view.text_input(cx, id).text().trim().into()
    }
    fn number(&self, cx: &Cx, id: &[LiveId], label: &str) -> Result<f64, String> {
        self.text(cx, id)
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("Enter a finite {label}"))
    }
    fn refresh(&mut self, cx: &mut Cx) -> Result<(), String> {
        let store = project();
        self.catalog = store.assets().map_err(|e| e.to_string())?;
        self.saved = store.scenes().map_err(|e| e.to_string())?;
        for a in &self.catalog {
            self.pinned.insert(
                Revision {
                    id: a.id.clone(),
                    revision: a.revision,
                },
                a.clone(),
            );
        }
        self.view
            .drop_down(cx, ids!(palette))
            .set_labels(cx, self.catalog.iter().map(|a| a.name.clone()).collect());
        let library = self.view.drop_down(cx, ids!(library));
        library.set_labels(
            cx,
            self.saved
                .iter()
                .map(|s| format!("{} · v{}", s.name, s.revision))
                .collect(),
        );
        if let Some(i) = self.saved.iter().position(|s| s.id == self.editor.scene.id) {
            library.set_selected_item(cx, i);
        }
        Ok(())
    }
    fn open(&mut self, cx: &mut Cx, scene: Scene) {
        self.rule_selected = None;
        for obj in &scene.instances {
            if !self.pinned.contains_key(&obj.asset) {
                match project().asset_revision(&obj.asset) {
                    Ok(a) => {
                        self.pinned.insert(obj.asset.clone(), a);
                    }
                    Err(e) => self.note(cx, &e.to_string()),
                }
            }
        }
        self.selected = scene.instances.first().map(|i| i.id.clone());
        self.baseline = scene.clone();
        self.editor.open(scene);
        self.candidate = None;
        self.review = Default::default();
        self.drag = None;
        self.sync(cx);
    }
    fn visible_scene(&self) -> &Scene {
        self.candidate.as_ref().unwrap_or(&self.editor.scene)
    }
    fn camera(&self) -> Camera {
        Camera::new(self.visible_scene().room_m, self.top)
    }
    fn draw_frame(&mut self, cx: &mut Cx) {
        let mut image = Viewport::render(
            self.visible_scene(),
            &self.pinned,
            self.camera(),
            self.selected.as_deref(),
        );
        let scene = self.visible_scene();
        if let Some(object) = scene
            .instances
            .iter()
            .find(|o| Some(&o.id) == self.selected.as_ref())
        {
            if let Some(area) = &object.start_area {
                if let Ok(corners) = scene.start_area_outline(object, area, &self.pinned) {
                    image.outline(self.camera(), corners);
                }
            }
        }
        VideoPlayerWidgetRefExt::as_video_player(&self.view.widget(cx, ids!(canvas))).set_frame(
            cx,
            WIDTH as usize,
            HEIGHT as usize,
            image.image.as_raw(),
        );
        self.frame = Some(image);
    }
    fn sync(&mut self, cx: &mut Cx) {
        let s = self.visible_scene();
        let mut selected = s
            .instances
            .iter()
            .position(|i| Some(&i.id) == self.selected.as_ref());
        if selected.is_none() && !s.instances.is_empty() {
            selected = Some(0);
        }
        let item = selected.map(|i| &s.instances[i]);
        let list = self.view.drop_down(cx, ids!(objects));
        list.set_labels(cx, s.instances.iter().map(|i| i.name.clone()).collect());
        list.set_selected_item(cx, selected.unwrap_or(0));
        let summary = if self.candidate.is_some() {
            format!(
                "Preview · {} objects · {} changed",
                s.instances.len(),
                changed_ids(&self.editor.scene, s).len()
            )
        } else {
            String::new()
        };
        self.view.label(cx, ids!(summary)).set_text(cx, &summary);
        let supports = self.supports();
        let support = self.view.drop_down(cx, ids!(support));
        let mut labels = vec!["Floor".to_string()];
        labels.extend(supports.iter().map(|(label, _)| label.clone()));
        support.set_labels(cx, labels.clone());
        let area = item.and_then(|i| i.start_area.as_ref());
        let selector = self.view.drop_down(cx, ids!(area_support));
        selector.set_labels(cx, labels);
        selector.set_selected_item(
            cx,
            area.and_then(|a| a.support.as_ref())
                .and_then(|link| supports.iter().position(|(_, s)| s == link))
                .map_or(0, |i| i + 1),
        );
        for (id, edge, axis) in [
            (ids!(area_x_min) as &[LiveId], 0, 0),
            (ids!(area_x_max), 1, 0),
            (ids!(area_y_min), 0, 1),
            (ids!(area_y_max), 1, 1),
        ] {
            let value = area
                .and_then(|a| a.bounds_m)
                .map(|b| b[edge][axis].to_string())
                .unwrap_or_default();
            self.view.text_input(cx, id).set_text(cx, &value);
        }
        self.view.label(cx, ids!(area_status)).set_text(
            cx,
            if area.is_some() {
                "Area defined · available in Run"
            } else {
                "No start area"
            },
        );
        support.set_selected_item(
            cx,
            item.and_then(|i| i.support.as_ref())
                .and_then(|link| supports.iter().position(|(_, s)| s == link))
                .map_or(0, |i| i + 1),
        );
        let issues = s.issues(&self.pinned);
        self.view.label(cx, ids!(validation)).set_text(
            cx,
            &issues
                .iter()
                .find(|issue| Some(issue.instance.as_str()) == item.map(|i| i.id.as_str()))
                .map(|issue| issue.message.clone())
                .unwrap_or_else(|| {
                    if issues.is_empty() {
                        "Geometry checks passed".into()
                    } else {
                        format!("{} layout issues", issues.len())
                    }
                }),
        );
        self.view
            .button(cx, ids!(next_issue))
            .set_enabled(cx, !issues.is_empty());
        let status = if self.candidate.is_some() {
            "Proposed layout"
        } else if self.editor.scene != self.baseline {
            "Unsaved edits"
        } else if s.revision == 0 {
            "New layout"
        } else {
            "Saved"
        };
        self.view.label(cx, ids!(header)).set_text(
            cx,
            &format!(
                "{}   ·   Revision {}   ·   {} objects   ·   {status}",
                s.name,
                s.revision,
                s.instances.len()
            ),
        );
        self.view.label(cx, ids!(viewpoint)).set_text(
            cx,
            if self.top {
                "Plan · 0.5 m grid · X red, Y green"
            } else {
                "3D · 0.5 m grid · X red, Y green"
            },
        );
        self.view
            .button(cx, ids!(top_view))
            .set_text(cx, if self.top { "3D view" } else { "Top view" });
        let p = item.map(|v| v.position_m).unwrap_or_default();
        for (id, value) in [
            (ids!(scene_name) as &[LiveId], s.name.clone()),
            (ids!(room_w), s.room_m[0].to_string()),
            (ids!(room_d), s.room_m[1].to_string()),
            (
                ids!(selected_name),
                item.map(|v| v.name.clone()).unwrap_or_default(),
            ),
            (ids!(x), p[0].to_string()),
            (ids!(y), p[1].to_string()),
            (ids!(z), p[2].to_string()),
            (
                ids!(yaw),
                item.map(|v| v.yaw_deg).unwrap_or_default().to_string(),
            ),
        ] {
            self.view.text_input(cx, id).set_text(cx, &value);
        }
        let pin = item
            .map(|o| {
                self.pinned
                    .get(&o.asset)
                    .map(|a| {
                        format!(
                            "{} · asset v{} · {} colliders",
                            a.name,
                            a.revision,
                            a.geometry.colliders.len()
                        )
                    })
                    .unwrap_or_else(|| "Pinned asset missing".into())
            })
            .unwrap_or_default();
        self.view.label(cx, ids!(pin)).set_text(cx, &pin);
        let pending = self.review_active();
        self.view
            .widget(cx, ids!(proposal_row))
            .set_visible(cx, pending);
        for id in [
            ids!(new_scene) as &[LiveId],
            ids!(save),
            ids!(save_as),
            ids!(undo),
            ids!(redo),
            ids!(apply_room),
            ids!(snap),
            ids!(reload),
            ids!(propose),
            ids!(discard),
        ] {
            self.view.button(cx, id).set_enabled(cx, !pending);
        }
        self.view
            .button(cx, ids!(open_scene))
            .set_enabled(cx, !pending && !self.saved.is_empty());
        self.view
            .button(cx, ids!(add))
            .set_enabled(cx, !pending && !self.catalog.is_empty());
        self.view
            .widget(cx, ids!(apply))
            .set_visible(cx, self.candidate.is_some());
        self.view
            .button(cx, ids!(update))
            .set_enabled(cx, item.is_some() && (!pending || self.candidate.is_some()));
        for id in [
            ids!(remove) as &[LiveId],
            ids!(duplicate),
            ids!(area_save),
            ids!(area_clear),
        ] {
            self.view
                .button(cx, id)
                .set_enabled(cx, !pending && item.is_some());
        }
        self.selected = item.map(|i| i.id.clone());
        self.rule_fields(cx);
        self.review_labels(cx);
        self.draw_frame(cx);
    }
    fn supports(&self) -> Vec<(String, Support)> {
        let mut rows = vec![];
        for obj in &self.visible_scene().instances {
            if Some(&obj.id) == self.selected.as_ref() {
                continue;
            }
            if let Some(a) = self.pinned.get(&obj.asset) {
                for surface in &a.geometry.surfaces {
                    rows.push((
                        format!("{} / {}", obj.name, surface.name),
                        Support {
                            instance: obj.id.clone(),
                            surface: surface.name.clone(),
                        },
                    ));
                }
            }
        }
        rows
    }
    fn commit(&mut self, cx: &mut Cx, layout: Scene) {
        self.candidate = None;
        self.editor.change(layout);
        self.sync(cx);
    }
    fn place(&mut self, cx: &mut Cx, xy: [f64; 2]) -> Result<(), String> {
        let index = self.view.drop_down(cx, ids!(palette)).selected_item();
        let asset = self
            .catalog
            .get(index)
            .ok_or("Create an asset in Survey & assets, then refresh the palette")?;
        let mut layout = self.editor.scene.clone();
        self.selected = Some(
            layout
                .place(asset, [xy[0], xy[1], 0.])
                .map_err(|e| e.to_string())?,
        );
        self.commit(cx, layout);
        Ok(())
    }
    fn pixel(&self, cx: &Cx, abs: Vec2d) -> Option<[f64; 2]> {
        let area = self
            .view
            .widget(cx, ids!(canvas.video_area.frame_image))
            .area()
            .rect(cx);
        if area.size.x <= 0. || area.size.y <= 0. || !area.contains(abs) {
            return None;
        }
        Some([
            (abs.x - area.pos.x) * WIDTH as f64 / area.size.x,
            (abs.y - area.pos.y) * HEIGHT as f64 / area.size.y,
        ])
    }
    fn actions(&mut self, cx: &mut Cx, a: &Actions) -> Result<(), String> {
        if !self.review_active()
            && self
                .view
                .text_input(cx, ids!(scene_name))
                .changed(a)
                .is_some()
        {
            let mut draft = self.editor.scene.clone();
            draft.name = self.text(cx, ids!(scene_name));
            self.editor.change(draft);
        }
        let click = |id: &[LiveId]| self.view.button(cx, id).clicked(a);
        if !self.review_active() && click(ids!(reload)) {
            self.refresh(cx)?;
            self.sync(cx);
            self.note(cx, "Library refreshed");
            return Ok(());
        }
        if click(ids!(top_view)) {
            self.top = !self.top;
            self.sync(cx);
            return Ok(());
        }
        if click(ids!(cancel)) {
            self.review = Default::default();
            self.candidate = None;
            self.sync(cx);
            self.note(cx, "Proposal discarded");
            return Ok(());
        }
        if click(ids!(apply)) {
            if let (Some(mut s), Some(mut source)) =
                (self.candidate.take(), self.review.source.take())
            {
                source.affected = changed_ids(&self.editor.scene, &s);
                self.review = Default::default();
                if source.affected.is_empty() {
                    self.sync(cx);
                    return Err("The reviewed layout is unchanged".into());
                }
                s.edit_history.push(source);
                self.commit(cx, s);
                self.note(cx, "Proposal applied");
            }
            return Ok(());
        }
        if click(ids!(confirm_reference)) {
            return self.confirm_reference(cx);
        }
        if !self.review_active() && click(ids!(propose)) {
            let text = self.text(cx, ids!(instruction));
            let provider = self
                .view
                .drop_down(cx, ids!(proposal_provider))
                .selected_item();
            if provider != 0 {
                return self.request_edit(
                    cx,
                    if provider == 1 {
                        "codex"
                    } else {
                        "octos-deepseek"
                    },
                    &text,
                );
            }
            let assets = if text.to_lowercase().trim_start().starts_with("move ") {
                self.pinned.values().cloned().collect::<Vec<_>>()
            } else {
                self.catalog.clone()
            };
            self.candidate = None;
            let result = self.editor.scene.propose(&text, &assets);
            match result {
                Ok(scene) => {
                    self.review.source = Some(EditSource::local(text, &self.editor.scene, &scene));
                    self.candidate = Some(scene);
                    self.sync(cx);
                    self.note(cx, "Review the local library match");
                }
                Err(e) => {
                    self.sync(cx);
                    return Err(e.to_string());
                }
            }
            return Ok(());
        }
        if self.review_active() && self.candidate.is_none() {
            return Ok(());
        }
        if let Some(i) = self.view.drop_down(cx, ids!(objects)).selected(a) {
            self.selected = self.visible_scene().instances.get(i).map(|v| v.id.clone());
            self.sync(cx);
            return Ok(());
        }
        if click(ids!(update)) {
            let name = self.text(cx, ids!(selected_name));
            if name.is_empty() {
                return Err("Name the instance".into());
            }
            let p = [
                self.number(cx, ids!(x), "X position")?,
                self.number(cx, ids!(y), "Y position")?,
                self.number(cx, ids!(z), "Z position")?,
            ];
            let mut s = self.visible_scene().clone();
            let id = self.selected.as_ref().ok_or("Select an object")?;
            let obj = s
                .instances
                .iter_mut()
                .find(|o| &o.id == id)
                .ok_or("Select an object")?;
            obj.name = name;
            obj.position_m = p;
            obj.yaw_deg = self.number(cx, ids!(yaw), "rotation in degrees")?;
            if self.candidate.is_some() {
                if Some(&s) != self.candidate.as_ref() {
                    let source = self
                        .review
                        .source
                        .as_mut()
                        .ok_or("Missing proposal source")?;
                    if !source.operator_adjusted.contains(id) {
                        source.operator_adjusted.push(id.clone());
                    }
                    self.candidate = Some(s);
                    self.sync(cx);
                    self.note(cx, "Preview pose updated");
                }
            } else {
                self.commit(cx, s);
            }
            return Ok(());
        }
        if self.candidate.is_some() {
            return Ok(());
        }
        if self.rule_actions(cx, a)? {
            return Ok(());
        }
        let click = |id: &[LiveId]| self.view.button(cx, id).clicked(a);
        if click(ids!(new_scene)) || click(ids!(open_scene)) || click(ids!(discard)) {
            if !click(ids!(discard)) && self.editor.scene != self.baseline {
                return Err("Save changes or reopen the saved layout first".into());
            }
            let scene = if click(ids!(new_scene)) {
                Scene::default()
            } else if click(ids!(discard)) {
                match self.baseline.reference() {
                    Some(pin) => project().scene_revision(&pin).map_err(|e| e.to_string())?,
                    None => Scene::default(),
                }
            } else {
                self.saved
                    .get(self.view.drop_down(cx, ids!(library)).selected_item())
                    .ok_or("No saved scene selected")?
                    .clone()
            };
            self.open(cx, scene);
            self.note(cx, "Layout opened");
            return Ok(());
        }
        if click(ids!(save)) || click(ids!(save_as)) {
            let mut scene = self.editor.scene.clone();
            scene.name = self.text(cx, ids!(scene_name));
            let snapshot = project()
                .save_scene(&scene, click(ids!(save_as)))
                .map_err(|e| e.to_string())?;
            self.editor.change(scene);
            self.editor.scene = snapshot.clone();
            self.baseline = snapshot;
            self.refresh(cx)?;
            self.sync(cx);
            self.note(cx, "Scene revision saved");
            return Ok(());
        }
        if click(ids!(snap)) {
            let index = self.view.drop_down(cx, ids!(support)).selected_item();
            let link = index
                .checked_sub(1)
                .and_then(|k| self.supports().get(k).map(|(_, link)| link.clone()));
            let id = self.selected.as_ref().ok_or("Select an object")?;
            let mut next = self.editor.scene.clone();
            next.snap_to(id, link, &self.pinned)
                .map_err(|e| e.to_string())?;
            self.commit(cx, next);
            return Ok(());
        }
        if click(ids!(area_save)) || click(ids!(area_clear)) {
            let id = self.selected.as_ref().ok_or("Select an object")?;
            let mut next = self.editor.scene.clone();
            let index = next
                .instances
                .iter()
                .position(|i| &i.id == id)
                .ok_or("Select an object")?;
            let area = if click(ids!(area_save)) {
                let fields = [
                    ids!(area_x_min) as &[LiveId],
                    ids!(area_y_min),
                    ids!(area_x_max),
                    ids!(area_y_max),
                ];
                let bounds_m = if fields.iter().all(|f| self.text(cx, f).is_empty()) {
                    None
                } else {
                    let n = |field| self.number(cx, field, "area boundary in metres");
                    Some([
                        [n(fields[0])?, n(fields[1])?],
                        [n(fields[2])?, n(fields[3])?],
                    ])
                };
                let support = self
                    .view
                    .drop_down(cx, ids!(area_support))
                    .selected_item()
                    .checked_sub(1)
                    .map(|i| {
                        self.supports()
                            .get(i)
                            .map(|(_, s)| s.clone())
                            .ok_or("Select a support")
                    })
                    .transpose()?;
                let area = StartArea { support, bounds_m };
                next.start_area_outline(&next.instances[index], &area, &self.pinned)
                    .map_err(|e| e.to_string())?;
                Some(area)
            } else {
                None
            };
            next.instances[index].start_area = area;
            self.commit(cx, next);
            return Ok(());
        }
        if click(ids!(next_issue)) {
            let issues = self.editor.scene.issues(&self.pinned);
            let i = issues
                .iter()
                .position(|v| Some(&v.instance) == self.selected.as_ref())
                .map_or(0, |n| (n + 1) % issues.len().max(1));
            if let Some(issue) = issues.get(i) {
                self.selected = Some(issue.instance.clone());
                self.sync(cx);
                self.note(cx, &issue.message);
            }
            return Ok(());
        }
        if click(ids!(undo)) || click(ids!(redo)) {
            if click(ids!(undo)) {
                self.editor.undo()
            } else {
                self.editor.redo()
            };
            self.sync(cx);
            return Ok(());
        }
        if click(ids!(add)) {
            return self.place(cx, [0., 0.]);
        }
        let mut s = self.editor.scene.clone();
        if click(ids!(apply_room)) {
            s.room_m = [
                self.number(cx, ids!(room_w), "room width")?,
                self.number(cx, ids!(room_d), "room depth")?,
            ];
            if s.room_m.iter().any(|n| *n <= 0.) {
                return Err("Room dimensions must be positive".into());
            }
            self.commit(cx, s);
            return Ok(());
        }
        let Some(index) = s
            .instances
            .iter()
            .position(|i| Some(&i.id) == self.selected.as_ref())
        else {
            return Ok(());
        };
        if click(ids!(remove)) {
            let removed = s.instances.remove(index);
            s.rules.retain(|r| r.instance.as_ref() != Some(&removed.id));
        } else if click(ids!(duplicate)) {
            let original = s.instances[index].clone();
            let a = self
                .pinned
                .get(&original.asset)
                .ok_or("Pinned asset is missing")?;
            let [x, y, z] = original.position_m;
            self.selected = Some(
                s.place(a, [x + 0.15, y + 0.15, z])
                    .map_err(|e| e.to_string())?,
            );
            let duplicate = s.instances.last_mut().unwrap();
            duplicate.yaw_deg = original.yaw_deg;
            duplicate.start_area = original.start_area;
        } else {
            return Ok(());
        }
        self.commit(cx, s);
        Ok(())
    }
}

impl Widget for SceneEditorPane {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if !self.view.visible {
            return;
        }
        self.poll_edit(cx);
        let tile = self.view.widget(cx, ids!(palette_tile)).area();
        match event.hits(cx, tile) {
            Hit::FingerDown(_) if !self.review_active() => {
                self.dragging_asset = true;
                self.note(cx, "Release over the room to place the selected asset");
                return;
            }
            Hit::FingerUp(e) if self.dragging_asset && !self.review_active() => {
                self.dragging_asset = false;
                if let Some([x, y]) = self.pixel(cx, e.abs) {
                    let p = self.camera().floor(x, y, 0.);
                    if let Err(e) = self.place(cx, p) {
                        self.note(cx, &e);
                    }
                }
                return;
            }
            _ => {}
        }
        let a = cx.capture_actions(|cx| self.view.handle_event(cx, event, scope));
        if let Err(e) = self.actions(cx, &a) {
            self.note(cx, &e);
        }
        if self.review_active() {
            return;
        }
        let frame = self
            .view
            .widget(cx, ids!(canvas.video_area.frame_image))
            .area();
        match event.hits(cx, frame) {
            Hit::FingerDown(e) => {
                let Some([x, y]) = self.pixel(cx, e.abs) else {
                    return;
                };
                cx.set_key_focus(frame);
                let Some(i) = self.frame.as_ref().and_then(|f| f.at(x, y)) else {
                    return;
                };
                let obj = &self.editor.scene.instances[i];
                let [u, v, z] = obj.position_m;
                let [a, b] = self.camera().floor(x, y, z);
                self.selected = Some(obj.id.clone());
                self.drag = Some(Drag {
                    before: self.editor.scene.clone(),
                    id: obj.id.clone(),
                    offset: [u - a, v - b],
                });
                self.sync(cx);
            }
            Hit::FingerMove(e) => {
                let Some([x, y]) = self.pixel(cx, e.abs) else {
                    return;
                };
                let Some(drag) = &self.drag else { return };
                let camera = self.camera();
                if let Some(obj) = self
                    .editor
                    .scene
                    .instances
                    .iter_mut()
                    .find(|v| v.id == drag.id)
                {
                    let [a, b] = camera.floor(x, y, obj.position_m[2]);
                    obj.position_m[0] = a + drag.offset[0];
                    obj.position_m[1] = b + drag.offset[1];
                    self.draw_frame(cx);
                }
            }
            Hit::FingerUp(_) => {
                if let Some(drag) = self.drag.take() {
                    let next = std::mem::replace(&mut self.editor.scene, drag.before);
                    self.commit(cx, next);
                }
            }
            Hit::KeyDown(k) => {
                if k.key_code == KeyCode::KeyZ && (k.modifiers.logo || k.modifiers.control) {
                    if k.modifiers.shift {
                        self.editor.redo();
                    } else {
                        self.editor.undo();
                    }
                    self.sync(cx);
                } else if matches!(k.key_code, KeyCode::Delete | KeyCode::Backspace) {
                    let mut next = self.editor.scene.clone();
                    next.instances
                        .retain(|i| Some(&i.id) != self.selected.as_ref());
                    self.commit(cx, next);
                } else if matches!(k.key_code, KeyCode::ArrowLeft | KeyCode::ArrowRight) {
                    let items = &self.editor.scene.instances;
                    if let Some(i) = items
                        .iter()
                        .position(|o| Some(&o.id) == self.selected.as_ref())
                    {
                        let next = if k.key_code == KeyCode::ArrowRight {
                            (i + 1) % items.len()
                        } else {
                            (i + items.len() - 1) % items.len()
                        };
                        self.selected = Some(items[next].id.clone());
                        self.sync(cx);
                    }
                }
            }
            _ => {}
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        if !self.loaded {
            self.loaded = true;
            match self.refresh(cx) {
                Ok(()) => self.open(cx, self.saved.first().cloned().unwrap_or_default()),
                Err(e) => self.note(cx, &e),
            }
        }
        self.view.draw_walk(cx, scope, walk)
    }
}

impl SceneEditorPaneRef {
    pub fn task_scene(&self, cx: &mut Cx) -> Option<Revision> {
        let w = self.borrow()?;
        if w.baseline != w.editor.scene || w.editor.scene.revision == 0 || w.review_active() {
            w.note(
                cx,
                "Apply your changes and save a scene revision before creating a task",
            );
            return None;
        }
        w.editor.scene.reference()
    }
}
