use dorobot_studio_core::{
    runtime,
    survey::{Photo, Survey},
    widgets::video_player::VideoPlayerWidgetRefExt,
};
use dorobot_studio_plugin::*;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    mod.widgets.SurveyPaneBase = #(SurveyPane::register_widget(vm))
    mod.widgets.SurveyPane = set_type_default() do mod.widgets.SurveyPaneBase{
        width: Fill height: Fill flow: Down spacing: 10
        View{width: Fill height: Fit flow: Right spacing: 10 align: Align{y:0.5}
            pick := mod.widgets.ux.PrimaryButton{text:"Import photos"}
            previous_photo := mod.widgets.ux.ToolButton{text:"Previous"}
            next_photo := mod.widgets.ux.ToolButton{text:"Next"}
            file_label := Label{width:Fill text:"Drop room or object photos here"}
        }
        photo_canvas := VideoPlayer{width:Fill height:Fill}
        import_result := Label{width:Fill max_lines:5 text:"JPEG and PNG · Drop files in the photo area"}
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct SurveyPane {
    #[deref]
    view: View,
    #[rust]
    loaded: bool,
    #[rust]
    photos: Vec<Photo>,
    #[rust]
    index: usize,
    #[rust]
    imports: Option<Receiver<Import>>,
    #[rust]
    queue: Option<Sender<Import>>,
}

enum Import {
    Finished(Vec<Photo>, Vec<String>),
}

fn project() -> Survey {
    Survey::new(runtime::user_data().join("aisle").join("survey"))
}

impl SurveyPane {
    fn show(&self, cx: &mut Cx) {
        let player = VideoPlayerWidgetRefExt::as_video_player(&self.view.widget(cx, ids!(photo_canvas)));
        if let Some(p) = self.photos.get(self.index) {
            self.view.label(cx, ids!(file_label)).set_text(
                cx,
                &format!(
                    "{} / {}   ·   {}   ·   {} × {} px",
                    self.index + 1,
                    self.photos.len(),
                    p.filename,
                    p.width,
                    p.height
                ),
            );
            match project().preview(p, false) {
                Ok(rgb) => player.set_frame(
                    cx,
                    rgb.width() as usize,
                    rgb.height() as usize,
                    rgb.as_raw(),
                ),
                Err(e) => {
                    player.clear(cx);
                    self.view
                        .label(cx, ids!(import_result))
                        .set_text(cx, &e.to_string());
                }
            }
        } else {
            player.clear(cx);
        }
        self.view
            .button(cx, ids!(previous_photo))
            .set_enabled(cx, self.index > 0);
        self.view
            .button(cx, ids!(next_photo))
            .set_enabled(cx, self.index + 1 < self.photos.len());
    }

    fn import(&mut self, paths: Option<Vec<PathBuf>>) {
        let sender = self
            .queue
            .get_or_insert_with(|| {
                let (tx, rx) = mpsc::channel();
                self.imports = Some(rx);
                tx
            })
            .clone();
        std::thread::spawn(move || {
            let Some(paths) = paths.or_else(|| {
                rfd::FileDialog::new()
                    .add_filter("Room and object photos", &["jpg", "jpeg", "png"])
                    .pick_files()
            }) else {
                return;
            };
            let store = project();
            let mut ok = Vec::new();
            let mut errors = Vec::new();
            for path in paths {
                match store.import(&path) {
                    Ok(photo) => ok.push(photo),
                    Err(e) => errors.push(format!(
                        "{} · {e}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )),
                }
            }
            let _ = sender.send(Import::Finished(ok, errors));
            SignalToUI::set_ui_signal();
        });
    }
}

impl Widget for SurveyPane {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if !self.view.visible {
            return;
        }
        let actions = cx.capture_actions(|cx| self.view.handle_event(cx, event, scope));
        let area = self.view.widget(cx, ids!(photo_canvas)).area();
        match event.drag_hits(cx, area) {
            DragHit::Drag(e)
                if e.items
                    .iter()
                    .any(|item| matches!(item, DragItem::FilePath { .. })) =>
            {
                *e.response.lock().unwrap() = DragResponse::Copy;
            }
            DragHit::Drop(e) => {
                let paths: Vec<_> = e
                    .items
                    .iter()
                    .filter_map(|item| {
                        if let DragItem::FilePath { path, .. } = item {
                            Some(PathBuf::from(path))
                        } else {
                            None
                        }
                    })
                    .collect();
                if !paths.is_empty() {
                    self.import(Some(paths));
                }
            }
            _ => {}
        }
        if self.view.button(cx, ids!(pick)).clicked(&actions) {
            self.import(None);
        }
        if self.view.button(cx, ids!(previous_photo)).clicked(&actions) && self.index > 0 {
            self.index -= 1;
            self.show(cx);
        }
        if self.view.button(cx, ids!(next_photo)).clicked(&actions)
            && self.index + 1 < self.photos.len()
        {
            self.index += 1;
            self.show(cx);
        }
        while let Some(Import::Finished(photos, errors)) =
            self.imports.as_ref().and_then(|rx| rx.try_recv().ok())
        {
            let selected = photos.last().map(|p| p.id.clone());
            match project().list() {
                Ok(photos) => self.photos = photos,
                Err(err) => {
                    self.view
                        .label(cx, ids!(import_result))
                        .set_text(cx, &err.to_string());
                    continue;
                }
            }
            if let Some(id) = selected {
                self.index = self.photos.iter().position(|p| p.id == id).unwrap_or(0);
            }
            let mut report = photos
                .iter()
                .map(|p| format!("Imported {}", p.filename))
                .collect::<Vec<_>>();
            report.extend(errors);
            self.view
                .label(cx, ids!(import_result))
                .set_text(cx, &report.join("\n"));
            self.show(cx);
        }
    }
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        if !self.loaded {
            self.loaded = true;
            match project().list() {
                Ok(photos) => self.photos = photos,
                Err(e) => self
                    .view
                    .label(cx, ids!(import_result))
                    .set_text(cx, &e.to_string()),
            }
            self.show(cx);
        }
        self.view.draw_walk(cx, scope, walk)
    }
}
