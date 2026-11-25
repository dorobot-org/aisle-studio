use dorobot_studio_core::{
    runtime,
    survey::{
        asset::Asset,
        scene::{
            validation::Support,
            viewport::{Camera, Viewport, HEIGHT, WIDTH},
            Revision, Scene,
        },
        task::BoundTask,
        Survey,
    },
    widgets::video_player::VideoPlayerWidgetRefExt,
};
use dorobot_studio_plugin::*;
use std::collections::HashMap;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    mod.widgets.TaskBindingBase = #(TaskBinding::register_widget(vm))
    let Row = View{width:Fill height:Fit flow:Right spacing:8 align:Align{y:0.5}}
    let LabelLine=Label{width:Fill max_lines:4}
    let Tool=mod.widgets.ux.ToolButton{}
    mod.widgets.TaskBinding=set_type_default() do mod.widgets.TaskBindingBase{
        width:Fill height:Fill flow:Down spacing:9
        Row{library := DropDown{width:Fill} reload_library := Tool{text:"Reload scenes"} earlier_scene := Tool{text:"Earlier revision"} later_scene := Tool{text:"Later revision"}}
        scene_label := LabelLine{}
        View{width:Fill height:Fill flow:Right spacing:12
            viewport := VideoPlayer{width:Fill height:Fill}
            ScrollYView{width:330 height:Fill flow:Down spacing:7
                LabelLine{text:"Nano pickup instruction"}
                bound_instruction := TextInput{width:Fill height:68 is_multiline:true}
                LabelLine{text:"Object to grasp"}
                pickup := DropDown{width:Fill}
                LabelLine{text:"Placement surface"}
                placement := DropDown{width:Fill}
                selection := LabelLine{}
                LabelLine{text:"Controller inputs · onboard RGB and encoders"}
                LabelLine{text:"Success · physical grasp, release and settled contact on the selected surface"}
            }
        }
        result := LabelLine{}
        Row{activate := mod.widgets.ux.PrimaryButton{text:"Use this task"} clear := Tool{text:"Use built-in scene"}}
    }
}
#[derive(Script, ScriptHook, Widget)]
pub struct TaskBinding {
    #[deref]
    view: View,
    #[rust]
    saved: Vec<Scene>,
    #[rust]
    scene: Option<Scene>,
    #[rust]
    assets: HashMap<Revision, Asset>,
    #[rust]
    pickup: Option<String>,
    #[rust]
    placement: Option<Support>,
    #[rust]
    pickup_choices: Vec<(String, String)>,
    #[rust]
    supports: Vec<(String, Support)>,
    #[rust]
    loaded: bool,
}
fn project() -> Survey {
    Survey::new(runtime::user_data().join("aisle/survey"))
}
impl TaskBinding {
    fn note(&self, cx: &mut Cx, m: &str) {
        self.view.label(cx, ids!(result)).set_text(cx, m);
    }
    fn refresh(&mut self, cx: &mut Cx, preferred: Option<Revision>) -> Result<(), String> {
        let p = project();
        self.saved = p.scenes().map_err(|e| e.to_string())?;
        self.view
            .drop_down(cx, ids!(library))
            .set_labels(cx, self.saved.iter().map(|s| s.name.clone()).collect());
        let active = p
            .active_task()
            .map_err(|e| e.to_string())?
            .map(|(_, f)| {
                std::fs::read(f).map_err(|e| e.to_string()).and_then(|b| {
                    serde_json::from_slice::<BoundTask>(&b).map_err(|e| e.to_string())
                })
            })
            .transpose()?;
        let preferred = preferred
            .or_else(|| {
                self.scene.as_ref().and_then(|s| {
                    self.saved
                        .iter()
                        .find(|latest| s.id == latest.id)
                        .and_then(Scene::reference)
                })
            })
            .or_else(|| active.as_ref().and_then(|a| a.scene.reference()))
            .or_else(|| self.saved.first().and_then(Scene::reference));
        if !self.loaded {
            self.view.text_input(cx, ids!(bound_instruction)).set_text(
                cx,
                active
                    .as_ref()
                    .map(|a| a.instruction.as_str())
                    .unwrap_or("Pick up the cube and put it on the coffee table."),
            );
            if let Some(t) = active
                .as_ref()
                .filter(|t| Some(&t.scene.id) == preferred.as_ref().map(|p| &p.id))
            {
                self.pickup = Some(t.pickup.clone());
                self.placement = Some(t.placement.clone());
            }
        }
        self.loaded = true;
        if let Some(pin) = preferred {
            self.open(cx, &pin)?;
        } else {
            self.note(cx, "Save a room in Scene editor first");
        }
        Ok(())
    }
    fn open(&mut self, cx: &mut Cx, pin: &Revision) -> Result<(), String> {
        let p = project();
        let s = p.scene_revision(pin).map_err(|e| e.to_string())?;
        let same = self.scene.as_ref().is_some_and(|old| old.id == s.id);
        if self.scene.is_some() && !same {
            self.pickup = None;
            self.placement = None;
        }
        self.assets = p.pinned_assets(&s).map_err(|e| e.to_string())?;
        self.pickup_choices.clear();
        self.supports.clear();
        for i in &s.instances {
            let asset = &self.assets[&i.asset];
            if asset.physics.graspable.value && asset.physics.movable.value {
                self.pickup_choices.push((i.name.clone(), i.id.clone()));
            }
            if !asset.physics.movable.value {
                for surface in &asset.geometry.surfaces {
                    self.supports.push((
                        format!("{} / {}", i.name, surface.name),
                        Support {
                            instance: i.id.clone(),
                            surface: surface.name.clone(),
                        },
                    ));
                }
            }
        }
        if self.pickup.is_none() && self.pickup_choices.len() == 1 {
            self.pickup = Some(self.pickup_choices[0].1.clone());
        }
        if self.placement.is_none() && self.supports.len() == 1 {
            self.placement = Some(self.supports[0].1.clone());
        }
        let library_index = self.saved.iter().position(|v| v.id == s.id).unwrap_or(0);
        self.view
            .drop_down(cx, ids!(library))
            .set_selected_item(cx, library_index);
        self.view
            .button(cx, ids!(earlier_scene))
            .set_enabled(cx, s.revision > 1);
        self.view.button(cx, ids!(later_scene)).set_enabled(
            cx,
            self.saved
                .get(library_index)
                .is_some_and(|t| t.revision > s.revision),
        );
        self.view.label(cx, ids!(scene_label)).set_text(
            cx,
            &format!(
                "{} · revision {} · {} instances",
                s.name,
                s.revision,
                s.instances.len()
            ),
        );
        self.scene = Some(s);
        self.sync(cx);
        Ok(())
    }
    fn sync(&self, cx: &mut Cx) {
        let Some(s) = &self.scene else { return };
        let pick = self
            .pickup_choices
            .iter()
            .position(|(_, id)| Some(id) == self.pickup.as_ref());
        let place = self
            .supports
            .iter()
            .position(|(_, link)| Some(link) == self.placement.as_ref());
        let pick_hint = if self.pickup.is_some() && pick.is_none() {
            "Bound pickup is missing"
        } else {
            "Choose object"
        };
        let surface_hint = if self.placement.is_some() && place.is_none() {
            "Bound surface is missing"
        } else {
            "Choose surface"
        };
        for (id, hint, names, selection) in [
            (
                ids!(pickup) as &[LiveId],
                pick_hint,
                self.pickup_choices
                    .iter()
                    .map(|x| x.0.clone())
                    .collect::<Vec<_>>(),
                pick,
            ),
            (
                ids!(placement),
                surface_hint,
                self.supports.iter().map(|x| x.0.clone()).collect(),
                place,
            ),
        ] {
            let chooser = self.view.drop_down(cx, id);
            chooser.set_labels(cx, std::iter::once(hint.to_string()).chain(names).collect());
            chooser.set_selected_item(cx, selection.map_or(0, |n| n + 1));
        }
        let picked = pick
            .map(|i| self.pickup_choices[i].0.as_str())
            .unwrap_or(pick_hint);
        let support = place
            .map(|i| self.supports[i].0.as_str())
            .unwrap_or(surface_hint);
        self.view
            .label(cx, ids!(selection))
            .set_text(cx, &format!("Pickup · {picked}\nSurface · {support}"));
        self.view
            .button(cx, ids!(activate))
            .set_enabled(cx, pick.is_some() && place.is_some());
        let message = if self.pickup.is_some() && pick.is_none() {
            "The bound pickup object was deleted from this revision. Choose a replacement."
        } else if self.placement.is_some() && place.is_none() {
            "The bound placement surface is missing from this revision. Choose a replacement."
        } else if pick.is_none() && self.pickup_choices.len() > 1 {
            "Multiple graspable objects. Choose the intended pickup object."
        } else if pick.is_none() {
            "The scene needs a movable, graspable object."
        } else if place.is_none() {
            "Choose the destination surface."
        } else {
            "Orange brackets mark the pickup. Teal marks the placement surface."
        };
        self.note(cx, message);
        let frame = Viewport::render_bindings(
            s,
            &self.assets,
            Camera::task_view(
                s,
                &self.assets,
                self.pickup.as_deref(),
                self.placement.as_ref().map(|p| p.instance.as_str()),
            ),
            self.pickup.as_deref(),
            self.placement.as_ref(),
        );
        VideoPlayerWidgetRefExt::as_video_player(&self.view.widget(cx, ids!(viewport))).set_frame(
            cx,
            WIDTH as usize,
            HEIGHT as usize,
            frame.image.as_raw(),
        );
    }
}
impl TaskBindingRef {
    pub fn browse(&self, cx: &mut Cx, pin: Option<Revision>) {
        if let Some(mut w) = self.borrow_mut() {
            if let Err(e) = w.refresh(cx, pin) {
                w.note(cx, &e);
            }
        }
    }
}
impl Widget for TaskBinding {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if !self.view.visible {
            return;
        }
        let actions = cx.capture_actions(|cx| self.view.handle_event(cx, event, scope));
        let result = (|| -> Result<(), String> {
            let click = |id: &[LiveId]| self.view.button(cx, id).clicked(&actions);
            if click(ids!(reload_library)) {
                return self.refresh(cx, None);
            }
            if click(ids!(clear)) {
                project().clear_active_task().map_err(|e| e.to_string())?;
                self.note(cx, "Built-in scene selected");
                return Ok(());
            }
            if let Some(i) = self.view.drop_down(cx, ids!(library)).selected(&actions) {
                if let Some(pin) = self.saved.get(i).and_then(Scene::reference) {
                    self.open(cx, &pin)?;
                }
            } else if click(ids!(earlier_scene)) || click(ids!(later_scene)) {
                if let Some(mut r) = self.scene.as_ref().and_then(Scene::reference) {
                    if click(ids!(earlier_scene)) {
                        r.revision = r.revision.saturating_sub(1)
                    } else {
                        r.revision += 1
                    };
                    self.open(cx, &r)?;
                }
            } else if let Some(i) = self.view.drop_down(cx, ids!(pickup)).selected(&actions) {
                self.pickup = i
                    .checked_sub(1)
                    .and_then(|i| self.pickup_choices.get(i))
                    .map(|v| v.1.clone());
                self.sync(cx);
            } else if let Some(i) = self.view.drop_down(cx, ids!(placement)).selected(&actions) {
                self.placement = i
                    .checked_sub(1)
                    .and_then(|i| self.supports.get(i))
                    .map(|v| v.1.clone());
                self.sync(cx);
            } else if click(ids!(activate)) {
                let pin = self
                    .scene
                    .as_ref()
                    .and_then(Scene::reference)
                    .ok_or("Choose a saved scene")?;
                let task = project()
                    .bind_pickup(
                        &pin,
                        &self.view.text_input(cx, ids!(bound_instruction)).text(),
                        self.pickup.as_deref().ok_or("Choose a pickup")?,
                        self.placement.as_ref().ok_or("Choose a surface")?,
                    )
                    .map_err(|e| e.to_string())?;
                let sha = project().activate_task(&task).map_err(|e| e.to_string())?;
                self.note(
                    cx,
                    &format!(
                        "Task {} saved · return to Task to prepare the controller",
                        &sha[..10]
                    ),
                );
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.note(cx, &e);
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}
