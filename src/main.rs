use dorobot_plugin_aisle::{
    scene_editor::SceneEditorPaneWidgetRefExt, task_binding::TaskBindingWidgetRefExt, AislePlugin,
};
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
    #[rust]
    binding_open: bool,
    #[rust]
    bound_status: Option<(String, String)>,
}

impl App {
    fn update_bound_status(&mut self) {
        let p = dorobot_studio_core::survey::Survey::new(
            dorobot_studio_core::runtime::user_data().join("aisle/survey"),
        );
        self.bound_status = match p.active_task() {
            Ok(Some((_, f))) => std::fs::read(f)
                .ok()
                .and_then(|d| {
                    serde_json::from_slice::<dorobot_studio_core::survey::task::BoundTask>(&d).ok()
                })
                .map(|t| {
                    (
                        format!(
                            "{} · revision {} · {}",
                            t.scene.name, t.scene.revision, t.instruction
                        ),
                        t.instruction,
                    )
                }),
            Ok(None) => None,
            Err(e) => Some((e.to_string(), String::new())),
        };
    }
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
        if self.pages.selected() == Some("aisle_task") {
            self.ui
                .widget(cx, ids!(aisle_task.project_task))
                .set_visible(cx, self.binding_open);
            self.ui.label(cx, ids!(aisle_task.task_heading)).set_text(
                cx,
                if self.binding_open {
                    "Bind scene task"
                } else {
                    "Robot task"
                },
            );
            if self.binding_open {
                self.ui.label(cx, ids!(aisle_task.context_label)).set_text(
                    cx,
                    "Adora Nano   /   AISLE · Nexus   /   Mobile manipulation",
                );
            }
            self.ui
                .widget(cx, ids!(aisle_task.row))
                .set_visible(cx, !self.binding_open);
            self.ui.button(cx, ids!(aisle_task.select_scene)).set_text(
                cx,
                if self.binding_open {
                    "Return to task"
                } else {
                    "Select scene"
                },
            );
            let nano = self
                .backend
                .aisle()
                .is_some_and(|w| w.robot == dorobot_studio_core::api::aisle::Robot::Nano);
            let active = self.bound_status.as_ref().filter(|_| nano);
            self.ui
                .widget(cx, ids!(aisle_task.binding_status))
                .set_visible(cx, !self.binding_open && active.is_some());
            self.ui
                .button(cx, ids!(aisle_task.select_scene))
                .set_enabled(cx, nano);
            if let Some((label, _)) = active {
                self.ui
                    .label(cx, ids!(aisle_task.binding_status))
                    .set_text(cx, label);
            }
            self.ui.button(cx, ids!(aisle_task.reference)).set_text(
                cx,
                if active.is_some() {
                    "Use reference controller"
                } else {
                    "Use reference task"
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
                        "aisle_scene.create_bound_task",
                        "aisle_task.binding_status",
                        "aisle_task.select_scene",
                        "aisle_task.project_task.library",
                        "aisle_task.project_task.reload_library",
                        "aisle_task.project_task.earlier_scene",
                        "aisle_task.project_task.later_scene",
                        "aisle_task.project_task.scene_label",
                        "aisle_task.project_task.bound_instruction",
                        "aisle_task.project_task.pickup",
                        "aisle_task.project_task.placement",
                        "aisle_task.project_task.selection",
                        "aisle_task.project_task.viewport",
                        "aisle_task.project_task.result",
                        "aisle_task.project_task.activate",
                        "aisle_task.project_task.clear",
                        "aisle_task.nano",
                        "aisle_task.g1",
                        "aisle_task.instruction",
                        "aisle_task.onboard",
                        "aisle_task.codex",
                        "aisle_task.octos",
                        "aisle_task.reference",
                        "aisle_run.start_run",
                        "aisle_run.experiment_mode",
                        "aisle_run.experiment_pane.exp_configs",
                        "aisle_run.experiment_pane.exp_load",
                        "aisle_run.experiment_pane.exp_save",
                        "aisle_run.experiment_pane.exp_start",
                        "aisle_run.experiment_pane.exp_batches",
                        "aisle_run.experiment_pane.exp_refresh",
                        "aisle_run.experiment_pane.exp_summary",
                        "aisle_run.experiment_pane.exp_rows",
                        "aisle_run.experiment_pane.exp_prev",
                        "aisle_run.experiment_pane.exp_next",
                        "aisle_run.experiment_pane.exp_cancel",
                        "aisle_run.experiment_pane.exp_retry",
                        "aisle_run.experiment_pane.exp_detail",
                        "aisle_run.experiment_pane.exp_form",
                        "aisle_run.experiment_pane.exp_name",
                        "aisle_run.experiment_pane.exp_count",
                        "aisle_run.experiment_pane.exp_seed_mode",
                        "aisle_run.experiment_pane.exp_seed",
                        "aisle_run.experiment_pane.exp_scope",
                        "aisle_run.experiment_pane.exp_trace",
                        "aisle_run.experiment_pane.exp_history",
                        "aisle_run.experiment_pane.exp_cameras",
                        "aisle_run.experiment_pane.exp_inherit",
                        "aisle_run.experiment_pane.exp_features",
                        "aisle_run.experiment_pane.exp_feature",
                        "aisle_run.experiment_pane.exp_type",
                        "aisle_run.experiment_pane.exp_units",
                        "aisle_run.experiment_pane.exp_binding",
                        "aisle_run.experiment_pane.exp_a",
                        "aisle_run.experiment_pane.exp_b",
                        "aisle_run.experiment_pane.exp_axis",
                        "aisle_run.experiment_pane.exp_default",
                        "aisle_run.experiment_pane.exp_low",
                        "aisle_run.experiment_pane.exp_high",
                        "aisle_run.experiment_pane.exp_choices",
                        "aisle_run.experiment_pane.exp_feature_save",
                        "aisle_run.experiment_pane.exp_feature_delete",
                        "aisle_run.experiment_pane.exp_feedback",
                        "aisle_run.experiment_pane.exp_context",
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
                        "aisle_scene.project_survey.appearance",
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
                        "aisle_evidence.ev_mode",
                        "aisle_evidence.ev_catalog.ev_search",
                        "aisle_evidence.ev_catalog.ev_filter",
                        "aisle_evidence.ev_catalog.ev_refresh",
                        "aisle_evidence.ev_catalog.ev_entries",
                        "aisle_evidence.ev_catalog.ev_identity",
                        "aisle_evidence.ev_catalog.ev_camera",
                        "aisle_evidence.ev_catalog.ev_clock",
                        "aisle_evidence.ev_catalog.ev_image",
                        "aisle_evidence.ev_catalog.ev_scrub",
                        "aisle_evidence.ev_catalog.ev_play",
                        "aisle_evidence.ev_catalog.ev_back",
                        "aisle_evidence.ev_catalog.ev_forward",
                        "aisle_evidence.ev_catalog.ev_phase",
                        "aisle_evidence.ev_catalog.ev_trace",
                        "aisle_evidence.ev_catalog.ev_channels",
                        "aisle_evidence.ev_catalog.ev_inspector",
                        "aisle_evidence.ev_catalog.ev_verdict",
                        "aisle_evidence.ev_catalog.ev_lineage",
                        "aisle_evidence.ev_catalog.ev_parent",
                        "aisle_evidence.ev_catalog.ev_original",
                        "aisle_evidence.ev_catalog.ev_name",
                        "aisle_evidence.ev_catalog.ev_notes",
                        "aisle_evidence.ev_catalog.ev_tags",
                        "aisle_evidence.ev_catalog.ev_fields",
                        "aisle_evidence.ev_catalog.ev_save_notes",
                        "aisle_evidence.ev_catalog.ev_revisions",
                        "aisle_evidence.ev_catalog.ev_revision_detail",
                        "aisle_evidence.ev_catalog.ev_variant_name",
                        "aisle_evidence.ev_catalog.ev_object",
                        "aisle_evidence.ev_catalog.ev_axis",
                        "aisle_evidence.ev_catalog.ev_position",
                        "aisle_evidence.ev_catalog.ev_seed",
                        "aisle_evidence.ev_catalog.ev_options",
                        "aisle_evidence.ev_catalog.ev_task_source",
                        "aisle_evidence.ev_catalog.ev_logs",
                        "aisle_evidence.ev_catalog.ev_capture",
                        "aisle_evidence.ev_catalog.ev_params",

                        "aisle_evidence.ev_catalog.ev_save_variant",
                        "aisle_evidence.ev_catalog.ev_variants",
                        "aisle_evidence.ev_catalog.ev_diff",
                        "aisle_evidence.ev_catalog.ev_edited",
                        "aisle_evidence.ev_catalog.ev_events",
                        "aisle_evidence.ev_catalog.ev_event_detail",
                        "aisle_evidence.ev_catalog.ev_feedback",
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
        self.update_bound_status();
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
        if self
            .ui
            .button(cx, ids!(aisle_scene.create_bound_task))
            .clicked(actions)
        {
            if let Some(pin) = self
                .ui
                .scene_editor_pane(cx, ids!(aisle_scene.composer))
                .task_scene(cx)
            {
                self.backend.dispatch(Intent::Aisle(
                    dorobot_studio_core::api::aisle::Action::Select(
                        dorobot_studio_core::api::aisle::Robot::Nano,
                    ),
                ));
                self.pages.select("aisle_task").expect("Task page");
                self.binding_open = true;
                self.sync(cx);
                self.ui
                    .task_binding(cx, ids!(aisle_task.project_task))
                    .browse(cx, Some(pin));
            }
        } else if self
            .ui
            .button(cx, ids!(aisle_task.select_scene))
            .clicked(actions)
        {
            self.binding_open = !self.binding_open;
            if self.binding_open {
                self.ui
                    .task_binding(cx, ids!(aisle_task.project_task))
                    .browse(cx, None);
            } else {
                self.update_bound_status();
                if let Some((_, task)) = &self.bound_status {
                    self.ui
                        .text_input(cx, ids!(aisle_task.instruction))
                        .set_text(cx, task);
                }
            }
        } else if let Some(page) = self.pages.tab_selection(actions) {
            self.binding_open = false;
            self.update_bound_status();
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
