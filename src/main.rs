use dorobot_plugin_aisle::AislePlugin;
use dorobot_studio_core::{
    api::{files::FileBackend, Backend, Intent, Screen},
    playback_controls::PlaybackAction,
    ui::{frame, play::PlayScreenWidgetRefExt},
    widgets::timeline::TimelineAction,
};
use dorobot_studio_plugin::{PanelContext, PluginRegistry};
use makepad_widgets::*;

app_main!(App, font_assets: [INTER_FONT_ASSET]);

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    startup() do #(App::script_component(vm)){
        ui: Root{
            main_window := Window{
                window.title: "AISLE Studio"
                window.inner_size: vec2(1280, 800)
                pass.clear_color: mod.theme.color_bg_app
                body +: {
                    flow: Down
                    header := View{
                        width: Fill height: 56 flow: Right align: Align{y: 0.5}
                        padding: Inset{left: 20 right: 20} spacing: 22
                        Label{text: "AISLE Studio" draw_text +: {text_style: mod.widgets.ux.TEXT_TITLE{}}}
                        tabs := View{width: Fit height: 44 flow: Right align: Align{y: 0.5} spacing: 4}
                        Filler{}
                        back := mod.widgets.ux.ToolButton{text: "Back to evidence" visible: false}
                    }
                    pages := View{width: Fill height: Fill flow: Overlay}
                    page_play := PlayScreen{width: Fill height: Fill visible: false}
                }
            }
        }
    }
}

fn registry() -> PluginRegistry {
    let mut pages = PluginRegistry::default();
    pages.register(Box::new(AislePlugin)).expect("AISLE pages");
    pages.select("aisle_task").expect("Task page");
    pages
}

fn backend() -> FileBackend {
    FileBackend::new(dorobot_studio_core::runtime::user_data().join("datasets"))
}

#[derive(Script, ScriptHook)]
struct App {
    #[live]
    ui: WidgetRef,
    #[rust(registry())]
    pages: PluginRegistry,
    #[rust(backend())]
    backend: FileBackend,
    #[rust]
    timer: Timer,
    #[rust]
    replay: bool,
    #[rust]
    last_tick: Option<std::time::Instant>,
}

impl App {
    fn sync(&mut self, cx: &mut Cx) {
        self.ui
            .widget(cx, ids!(pages))
            .set_visible(cx, !self.replay);
        self.ui
            .widget(cx, ids!(page_play))
            .set_visible(cx, self.replay);
        self.ui.widget(cx, ids!(back)).set_visible(cx, self.replay);
        self.pages.sync_navigation(cx, Screen::Library);
        if self.replay {
            self.ui
                .play_screen(cx, ids!(page_play))
                .sync(cx, self.backend.playback());
        } else {
            self.pages.sync(
                cx,
                &PanelContext {
                    backend: &self.backend,
                    rig_editing: false,
                    show_help: false,
                    show_details: false,
                    live_controls: true,
                },
            );
        }
        if std::env::var_os("DOROBOT_UX_REPORT").is_some() {
            if let Some(w) = self.backend.aisle() {
                dorobot_studio_core::ux_probe::write(
                    cx,
                    &self.ui,
                    serde_json::json!({"app":"AISLE Studio","page":self.pages.selected(),"replay":self.replay,
                        "busy":w.busy(),"status":w.status,"controller":w.plan,"result":w.evidence,
                        "selected":w.selected,"progress":w.progress}),
                    &[
                        "tabs.plugin_tab_aisle_task",
                        "tabs.plugin_tab_aisle_scene",
                        "tabs.plugin_tab_aisle_run",
                        "tabs.plugin_tab_aisle_evidence",
                        "aisle_task.nano",
                        "aisle_task.g1",
                        "aisle_task.instruction",
                        "aisle_task.codex",
                        "aisle_task.octos",
                        "aisle_task.reference",
                        "aisle_run.start_run",
                        "aisle_scene.inspect",
                        "aisle_scene.asset_mode",
                        "aisle_scene.composer_mode",
                        "aisle_scene.composer.library",
                        "aisle_scene.composer.open_scene",
                        "aisle_scene.composer.new_scene",
                        "aisle_scene.composer.scene_name",
                        "aisle_scene.composer.save",
                        "aisle_scene.composer.save_as",
                        "aisle_scene.composer.header",
                        "aisle_scene.composer.top_view",
                        "aisle_scene.composer.undo",
                        "aisle_scene.composer.redo",
                        "aisle_scene.composer.viewpoint",
                        "aisle_scene.composer.canvas",
                        "aisle_scene.composer.canvas.video_area.frame_image",
                        "aisle_scene.composer.instruction",
                        "aisle_scene.composer.propose",
                        "aisle_scene.composer.summary",
                        "aisle_scene.composer.apply",
                        "aisle_scene.composer.cancel",
                        "aisle_scene.composer.room_w",
                        "aisle_scene.composer.room_d",
                        "aisle_scene.composer.apply_room",
                        "aisle_scene.composer.palette",
                        "aisle_scene.composer.reload",
                        "aisle_scene.composer.palette_tile",
                        "aisle_scene.composer.add",
                        "aisle_scene.composer.inspector",
                        "aisle_scene.composer.objects",
                        "aisle_scene.composer.selected_name",
                        "aisle_scene.composer.x",
                        "aisle_scene.composer.y",
                        "aisle_scene.composer.z",
                        "aisle_scene.composer.yaw",
                        "aisle_scene.composer.update",
                        "aisle_scene.composer.duplicate",
                        "aisle_scene.composer.remove",
                        "aisle_scene.composer.pin",
                        "aisle_scene.composer.support",
                        "aisle_scene.composer.snap",
                        "aisle_scene.composer.next_issue",
                        "aisle_scene.composer.validation",
                        "aisle_scene.composer.discard",
                        "aisle_scene.composer.feedback",
                        "aisle_scene.project_survey.pick",
                        "aisle_scene.project_survey.edit_assets",
                        "aisle_scene.project_survey.room_name",
                        "aisle_scene.project_survey.assign_room",
                        "aisle_scene.project_survey.asset_name",
                        "aisle_scene.project_survey.category",
                        "aisle_scene.project_survey.outline",
                        "aisle_scene.project_survey.floor_ref",
                        "aisle_scene.project_survey.dimension",
                        "aisle_scene.project_survey.axis_label",
                        "aisle_scene.project_survey.next_axis",
                        "aisle_scene.project_survey.previous_axis",
                        "aisle_scene.project_survey.length_value",
                        "aisle_scene.project_survey.length_unit",
                        "aisle_scene.project_survey.apply_length",
                        "aisle_scene.project_survey.clear_marks",
                        "aisle_scene.project_survey.save_asset",
                        "aisle_scene.project_survey.asset_version",
                        "aisle_scene.project_survey.mark_hint",
                        "aisle_scene.project_survey.measured_values",
                        "aisle_scene.project_survey.asset_feedback",
                        "aisle_scene.project_survey.edit_marks",
                        "aisle_scene.project_survey.edit_physics",
                        "aisle_scene.project_survey.asset_shape",
                        "aisle_scene.project_survey.movable",
                        "aisle_scene.project_survey.material",
                        "aisle_scene.project_survey.friction",
                        "aisle_scene.project_survey.restitution",
                        "aisle_scene.project_survey.mass",
                        "aisle_scene.project_survey.graspable",
                        "aisle_scene.project_survey.surface_name",
                        "aisle_scene.project_survey.top_thickness",
                        "aisle_scene.project_survey.leg_width",
                        "aisle_scene.project_survey.geometry_view",
                        "aisle_scene.project_survey.geometry_info",
                        "aisle_scene.project_survey.rotate_geometry",
                        "aisle_scene.project_survey.form_scroll",
                        "aisle_scene.project_survey.preview_physics",
                        "aisle_scene.project_survey.new_asset",
                        "aisle_scene.project_survey.older_asset",
                        "aisle_scene.project_survey.next_asset",
                        "aisle_scene.project_survey.photo_canvas.video_area.frame_image",
                        "aisle_scene.project_survey.thumb0.select",
                        "aisle_scene.project_survey.thumb1.select",
                        "aisle_scene.project_survey.thumb2.select",
                        "aisle_scene.project_survey.photo_canvas",
                        "aisle_scene.project_survey.file_label",
                        "aisle_scene.project_survey.import_result",
                        "aisle_scene.project_survey.previous_photo",
                        "aisle_scene.project_survey.next_photo",
                        "aisle_scene.compact_visuals",
                        "aisle_evidence.verdict",
                        "aisle_evidence.export",
                        "aisle_evidence.replay",
                        "back",
                    ],
                );
            }
        }
    }
}

impl MatchEvent for App {
    fn handle_startup(&mut self, cx: &mut Cx) {
        frame::set_light_mode(1.0);
        self.pages
            .mount(cx, &self.ui.widget(cx, ids!(pages)))
            .expect("Mount pages");
        self.pages
            .mount_tabs(cx, &self.ui.widget(cx, ids!(tabs)))
            .expect("Mount navigation");
        self.timer = cx.start_interval(1.0 / 30.0);
        self.sync(cx);
    }

    fn handle_timer(&mut self, cx: &mut Cx, event: &TimerEvent) {
        if self.timer.is_timer(event).is_none() {
            return;
        }
        self.backend.poll();
        let now = std::time::Instant::now();
        if let Some(previous) = self.last_tick.replace(now) {
            let state = self.backend.playback();
            if self.replay && state.is_playing {
                let t = state.current_time + (now - previous).as_secs_f64() * state.speed;
                let end = state.stats.duration_s;
                self.backend.dispatch(Intent::Seek(t.min(end)));
                if t >= end {
                    self.backend.dispatch(Intent::TogglePlay);
                }
            }
        }
        self.sync(cx);
    }

    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        if let Some(page) = self.pages.tab_selection(actions) {
            self.replay = false;
            self.pages.select(page.id).expect("Registered page");
        } else if self.ui.button(cx, ids!(back)).clicked(actions) {
            self.replay = false;
            self.pages.select("aisle_evidence").expect("Evidence page");
        } else if self.replay {
            for action in actions.iter().filter_map(|a| a.as_widget_action()) {
                let intent = match action.cast::<TimelineAction>() {
                    TimelineAction::Seek(t) => Some(Intent::Seek(t)),
                    TimelineAction::SpeedChanged(v) => Some(Intent::SetSpeed(v)),
                    TimelineAction::Play | TimelineAction::Pause => Some(Intent::TogglePlay),
                    TimelineAction::StepForward => Some(Intent::StepFrames(1)),
                    TimelineAction::StepBackward => Some(Intent::StepFrames(-1)),
                    _ => match action.cast::<PlaybackAction>() {
                        PlaybackAction::Play | PlaybackAction::Pause => Some(Intent::TogglePlay),
                        PlaybackAction::StepForward => Some(Intent::StepFrames(1)),
                        PlaybackAction::StepBackward => Some(Intent::StepFrames(-1)),
                        _ => None,
                    },
                };
                if let Some(i) = intent {
                    self.backend.dispatch(i);
                }
            }
            if let Some(ep) = self
                .ui
                .play_screen(cx, ids!(page_play))
                .clicked_episode(cx, actions)
            {
                self.backend.dispatch(Intent::SelectEpisode(ep));
            }
        } else {
            let open = ["replay", "previous_change", "current_change"]
                .iter()
                .any(|name| {
                    self.ui
                        .button(cx, &[live_id!(aisle_evidence), LiveId::from_str(name)])
                        .clicked(actions)
                });
            self.pages.handle_actions(cx, actions, &mut self.backend);
            self.replay = open
                && self.backend.screen() == Screen::Play
                && self.backend.dataset_id().is_some();
        }
        self.sync(cx);
    }
}

impl AppMain for App {
    fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
        dorobot_studio_core::register_ui(vm);
        registry().register_widgets(vm);
        self::script_mod(vm)
    }
    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());
    }
}
