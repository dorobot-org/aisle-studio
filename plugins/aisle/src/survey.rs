mod editor;
use dorobot_studio_core::{
    runtime,
    survey::{Photo, PhotoPixels, Survey},
    widgets::video_player::VideoPlayerWidgetRefExt,
};
use dorobot_studio_plugin::*;
use editor::AssetEditor;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*
    mod.widgets.SurveyPaneBase = #(SurveyPane::register_widget(vm))
    let Field = TextInput{width:Fill height:32}
    let Tool = mod.widgets.ux.ToolButton{}
    let Line = View{width:Fill height:Fit flow:Right spacing:5 align:Align{y:0.5}}
    let Caption = Label{width:Fill max_lines:4}
    let PhotoTile = View{width: 160 height: 90 flow: Down spacing: 3
        thumb := VideoPlayer{width: Fill height: Fill}
        select := mod.widgets.ux.ToolButton{width:Fill height:25 text:""}
    }
    mod.widgets.SurveyPane = set_type_default() do mod.widgets.SurveyPaneBase{
        width: Fill height: Fill flow: Down spacing: 10
        View{width: Fill height: Fit flow: Right spacing: 10 align: Align{y:0.5}
            pick := mod.widgets.ux.PrimaryButton{text:"Import photos"}
            previous_photo := mod.widgets.ux.ToolButton{text:"Previous"}
            edit_assets := Tool{text:"Edit assets"}
            next_photo := mod.widgets.ux.ToolButton{text:"Next"}
            file_label := Label{width:Fill text:"Drop room or object photos here"}
        }
        thumb_bar := View{width:Fill height:Fit flow:Right spacing:8
            thumb0 := PhotoTile{} thumb1 := PhotoTile{} thumb2 := PhotoTile{}
            Filler{}
            room_link := View{width:240 height:Fit flow:Down spacing:4
                Label{text:"Room for selected photo"}
                Line{room_name := Field{empty_text:"Room name (optional)"} assign_room := Tool{text:"Apply"}}
            }
        }
        workspace := View{width:Fill height:Fill flow:Right spacing:10
            preview_area := View{width:Fill height:Fill flow:Overlay
                photo_canvas := VideoPlayer{width:Fill height:Fill}
                geometry_panel := View{visible:false width:Fill height:Fill flow:Down spacing:4
                    geometry_heading := Caption{text:"Collision geometry"}
                    geometry_view := VideoPlayer{width:Fill height:Fill}
                    geometry_info := Caption{}
                }
            }
            asset_inspector := View{visible:false width:294 height:Fill spacing:5 padding:6 flow:Down
                asset_version := Caption{text:"New asset"}
                form_scroll := ScrollYView{width:Fill height:Fill spacing:6 flow:Down
                Line{older_asset := Tool{text:"Previous asset"} next_asset := Tool{text:"Next"} new_asset := Tool{text:"New"}}
                asset_name := Field{empty_text:"Asset name"} category := Field{empty_text:"Category"}
                Line{edit_marks := Tool{text:"Photo marks"} edit_physics := Tool{text:"Physics"} edit_suggestions := Tool{text:"Suggestions"}}
                suggest_panel := View{visible:false width:Fill height:Fit flow:Down spacing:5
                    suggest_instruction := Field{empty_text:"Describe the target in the selected photo"}
                    suggest_scale := Field{empty_text:"Coplanar ruler length (m), optional"}
                    suggest_axis := Caption{}
                    Line{suggest_codex := Tool{text:"Ask Codex"} suggest_octos := Tool{text:"Ask DeepSeek"}}
                    suggest_groups := DropDown{width:Fill}
                    suggest_source := Caption{}
                    Line{suggest_photo := Tool{text:"Show source photo"} suggest_outline := Tool{text:"Edit outline"}}
                    suggest_fields := DropDown{width:Fill}
                    suggest_value := Field{empty_text:"Review or correct this value"}
                    Line{suggest_apply := Tool{text:"Use value"} suggest_reject := Tool{text:"Discard value"}}
                    suggest_status := Caption{}
                }
                marks_panel := View{width:Fill height:Fit flow:Down spacing:5
                    Line{outline := Tool{text:"Outline"} floor_ref := Tool{text:"Floor points"} dimension := Tool{text:"Ruler"}}
                    mark_hint := Caption{}
                    Line{previous_axis := Tool{text:"‹"} axis_label := Label{text:"Width"} next_axis := Tool{text:"›"}}
                    Line{length_value := Field{empty_text:"Known length"} length_unit := Tool{text:"cm"} apply_length := Tool{text:"Apply"}}
                    measured_values := Caption{}
                    clear_marks := Tool{text:"Clear selected marks"}
                }
                physics_panel := View{visible:false width:Fill height:Fit flow:Down spacing:4
                    Line{asset_shape := Tool{text:"Shape · box"} movable := Tool{text:"Fixed"}}
                    graspable := Tool{text:"Enable grasping"}
                    appearance := Tool{text:"Appearance · neutral"}
                    Line{Label{text:"Material"} material := Field{empty_text:"Material"}}
                    Line{Label{text:"Friction"} friction := Field{} Label{text:"Bounce"} restitution := Field{}}
                    mass_row := Line{Label{text:"Mass (kg)"} mass := Field{empty_text:"kg"}}
                    table_details := View{width:Fill height:Fit flow:Down spacing:3
                        Line{Label{text:"Surface"} surface_name := Field{empty_text:"Placement surface name"}}
                        Line{Label{text:"Top (m)"} top_thickness := Field{} Label{text:"Leg (m)"} leg_width := Field{}}
                    }
                    Line{preview_physics := Tool{text:"Preview changes"} rotate_geometry := Tool{text:"Rotate"}}
                }
                }
                Line{save_asset := mod.widgets.ux.PrimaryButton{text:"Save asset revision"} discard_edits := Tool{text:"Discard edits"}}
                asset_feedback := Caption{}
            }
        }
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
    editor: AssetEditor,
    #[rust]
    preview_cache: Option<(String, PhotoPixels)>,
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
    fn show(&mut self, cx: &mut Cx) {
        self.update_scale_context(cx);
        self.view
            .widget(cx, ids!(thumb_bar))
            .set_visible(cx, !self.photos.is_empty());
        for (offset, id) in [ids!(thumb0) as &[LiveId], ids!(thumb1), ids!(thumb2)]
            .iter()
            .enumerate()
        {
            let tile = self.view.widget(cx, id);
            let photo = self.photos.get(self.index / 3 * 3 + offset);
            tile.set_visible(cx, photo.is_some());
            if let Some(photo) = photo {
                tile.button(cx, ids!(select)).set_text(cx, &photo.filename);
                if let Ok(rgb) = project().preview(photo, true) {
                    VideoPlayerWidgetRefExt::as_video_player(&tile.widget(cx, ids!(thumb)))
                        .set_frame(
                            cx,
                            rgb.width() as usize,
                            rgb.height() as usize,
                            rgb.as_raw(),
                        );
                }
            }
        }
        self.view
            .button(cx, ids!(previous_photo))
            .set_enabled(cx, self.index > 0);
        self.view
            .button(cx, ids!(next_photo))
            .set_enabled(cx, self.index + 1 < self.photos.len());
        self.show_photo(cx);
    }

    fn show_photo(&mut self, cx: &mut Cx) {
        let player =
            VideoPlayerWidgetRefExt::as_video_player(&self.view.widget(cx, ids!(photo_canvas)));
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
            if !self
                .preview_cache
                .as_ref()
                .is_some_and(|(id, _)| id == &p.id)
            {
                match project().room(p) {
                    Ok(room) => self
                        .view
                        .text_input(cx, ids!(room_name))
                        .set_text(cx, &room),
                    Err(e) => self
                        .view
                        .label(cx, ids!(import_result))
                        .set_text(cx, &e.to_string()),
                }
                match project().preview(p, false) {
                    Ok(rgb) => self.preview_cache = Some((p.id.clone(), rgb)),
                    Err(e) => {
                        self.preview_cache = None;
                        player.clear(cx);
                        self.view
                            .label(cx, ids!(import_result))
                            .set_text(cx, &e.to_string());
                        return;
                    }
                }
            }
            if let Some((_, original)) = &self.preview_cache {
                let mut rgb = original.clone();
                if self.editor.open {
                    if let Some(mark) = self.editor.asset.photos.get(&p.id) {
                        mark.paint(&mut rgb, p, self.editor.axis);
                    }
                    if let Some(outline) = self.editor.proposed_outline(&p.id) {
                        let overlay = dorobot_studio_core::survey::asset::Markup {
                            outline: outline.to_vec(),
                            ..Default::default()
                        };
                        overlay.paint_with_outline(&mut rgb, p, self.editor.axis, [237, 178, 21]);
                    }
                }
                player.set_frame(
                    cx,
                    rgb.width() as usize,
                    rgb.height() as usize,
                    rgb.as_raw(),
                );
            }
        } else {
            player.clear(cx);
        }
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
        for (offset, id) in [ids!(thumb0) as &[LiveId], ids!(thumb1), ids!(thumb2)]
            .iter()
            .enumerate()
        {
            let index = self.index / 3 * 3 + offset;
            if index >= self.photos.len() {
                break;
            }
            let tile = self.view.widget(cx, id);
            if tile.button(cx, ids!(select)).clicked(&actions)
                || matches!(
                    event.hits(cx, tile.widget(cx, ids!(thumb)).area()),
                    Hit::FingerUp(_)
                )
            {
                self.index = index;
                self.show(cx);
                break;
            }
        }
        if self.view.button(cx, ids!(assign_room)).clicked(&actions) {
            if let Some(photo) = self.photos.get(self.index) {
                let name = self.view.text_input(cx, ids!(room_name)).text();
                let result = project().assign_room(photo, &name);
                let message = match result {
                    Ok(()) if name.trim().is_empty() => "Room association cleared".into(),
                    Ok(()) => format!("{} belongs to {}", photo.filename, name.trim()),
                    Err(e) => e.to_string(),
                };
                self.view
                    .label(cx, ids!(import_result))
                    .set_text(cx, &message);
            }
        }
        self.handle_editor(cx, event, &actions);
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
