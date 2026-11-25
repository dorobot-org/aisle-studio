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
                        "aisle_scene.project_survey.pick",
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
