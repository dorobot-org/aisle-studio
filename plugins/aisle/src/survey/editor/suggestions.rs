use super::*;
use dorobot_studio_core::runtime;
use dorobot_studio_core::survey::suggestion::{Decision, Suggestion};
use serde_json::json;
use std::{io::Write, process::Stdio, sync::mpsc};

#[derive(Default)]
pub(super) struct Review {
    pub(super) open: bool,
    group: usize,
    field: usize,
    scale_context: Option<(String, usize)>,
    pending: Option<mpsc::Receiver<Result<Suggestion, String>>>,
}

impl AssetEditor {
    pub(in crate::survey) fn proposed_outline(&self, photo: &str) -> Option<&[Pixel]> {
        self.asset
            .suggestions
            .get(self.review.group)
            .filter(|s| self.review.open && s.photo == photo)
            .map(|s| s.outline.as_slice())
    }
}
impl SurveyPane {
    pub(in crate::survey) fn update_scale_context(&mut self, cx: &mut Cx) {
        let context = self
            .photos
            .get(self.index)
            .map(|p| (p.id.clone(), self.editor.axis));
        if context != self.editor.review.scale_context {
            self.view
                .text_input(cx, ids!(suggest_scale))
                .set_text(cx, "");
            self.editor.review.scale_context = context;
        }
    }
    pub(super) fn suggestion_labels(&self, cx: &mut Cx) {
        let e = &self.editor;
        self.view.label(cx,ids!(suggest_axis)).set_text(cx,&format!("For an approximate {} in metres, use a straight-on photo with the ruler beside the outline in the same plane.",AXES[e.axis].to_lowercase()));
        let groups = self.view.drop_down(cx, ids!(suggest_groups));
        groups.set_labels(
            cx,
            e.asset
                .suggestions
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{} · {}", i + 1, s.model))
                .collect(),
        );
        groups.set_selected_item(cx, e.review.group);
        let s = e.asset.suggestions.get(e.review.group);
        let field = s.and_then(|s| s.fields.get(e.review.field));
        let fields = self.view.drop_down(cx, ids!(suggest_fields));
        fields.set_labels(
            cx,
            s.into_iter()
                .flat_map(|s| s.fields.iter())
                .map(|f| f.field.clone())
                .collect(),
        );
        fields.set_selected_item(cx, e.review.field);
        let waiting = e.review.pending.is_some();
        for id in [ids!(suggest_codex) as &[LiveId], ids!(suggest_octos)] {
            self.view
                .button(cx, id)
                .set_enabled(cx, !waiting && self.photos.get(self.index).is_some());
        }
        let source = s
            .map(|s| {
                let name = self
                    .photos
                    .iter()
                    .find(|p| p.id == s.photo)
                    .map(|p| p.filename.as_str())
                    .unwrap_or("Missing photo");
                format!(
                    "{name} · {}\n{} · yellow outline\n{}",
                    s.model, s.input_mode, s.reason
                )
            })
            .unwrap_or_default();
        self.view
            .label(cx, ids!(suggest_source))
            .set_text(cx, &source);
        for id in [ids!(suggest_photo) as &[LiveId], ids!(suggest_outline)] {
            self.view.button(cx, id).set_enabled(cx, s.is_some());
        }
        self.view.text_input(cx, ids!(suggest_value)).set_text(
            cx,
            field
                .map(|f| f.applied.as_ref().unwrap_or(&f.value).as_str())
                .unwrap_or(""),
        );
        let label = field
            .map(|f| {
                let unit = if matches!(f.field.as_str(), "width" | "depth" | "height") {
                    " m"
                } else {
                    ""
                };
                format!("{:?} · proposed {}{unit}", f.status, f.value)
            })
            .unwrap_or_default();
        self.view
            .label(cx, ids!(suggest_status))
            .set_text(cx, &label);
        for id in [ids!(suggest_apply) as &[LiveId], ids!(suggest_reject)] {
            self.view
                .button(cx, id)
                .set_enabled(cx, field.is_some_and(|f| f.status == Decision::Pending));
        }
    }

    fn open_suggestion_photo(&mut self, cx: &mut Cx) {
        if let Some(i) = self
            .editor
            .asset
            .suggestions
            .get(self.editor.review.group)
            .and_then(|s| self.photos.iter().position(|p| p.id == s.photo))
        {
            self.index = i;
            self.show(cx);
        }
    }

    pub(super) fn suggestion_actions(
        &mut self,
        cx: &mut Cx,
        actions: &Actions,
    ) -> Result<(), String> {
        let clicked = |id: &[LiveId]| self.view.button(cx, id).clicked(actions);
        if clicked(ids!(suggest_codex)) || clicked(ids!(suggest_octos)) {
            let provider = if clicked(ids!(suggest_codex)) {
                "codex"
            } else {
                "octos-deepseek"
            };
            return self.request_suggestion(cx, provider);
        }
        if let Some(i) = self
            .view
            .drop_down(cx, ids!(suggest_groups))
            .selected(actions)
        {
            self.editor.review.group = i;
            self.editor.review.field = 0;
            self.open_suggestion_photo(cx);
            self.suggestion_labels(cx);
        }
        if self.view.button(cx, ids!(suggest_photo)).clicked(actions) {
            self.open_suggestion_photo(cx);
        }
        if self.view.button(cx, ids!(suggest_outline)).clicked(actions) {
            self.open_suggestion_photo(cx);
            let e = &mut self.editor;
            let s = e
                .asset
                .suggestions
                .get(e.review.group)
                .ok_or("Choose a proposal")?;
            e.asset
                .photos
                .entry(s.photo.clone())
                .or_default()
                .outline
                .clone_from(&s.outline);
            e.review.open = false;
            e.tool = Tool::Outline;
            e.dirty = true;
            self.editor_labels(cx);
            self.show_photo(cx);
            self.note(cx, "Drag the outline handles and save the asset revision");
            return Ok(());
        }
        if let Some(i) = self
            .view
            .drop_down(cx, ids!(suggest_fields))
            .selected(actions)
        {
            self.editor.review.field = i;
            self.suggestion_labels(cx);
        }
        let use_value = self.view.button(cx, ids!(suggest_apply)).clicked(actions);
        if use_value || self.view.button(cx, ids!(suggest_reject)).clicked(actions) {
            let edit = self.field(cx, ids!(suggest_value));
            self.open_suggestion_photo(cx);
            let e = &mut self.editor;
            let field = e
                .asset
                .suggestions
                .get(e.review.group)
                .ok_or("Choose a proposal")?
                .fields
                .get(e.review.field)
                .ok_or("Choose a property")?
                .field
                .clone();
            let decision = e
                .asset
                .review_field(
                    e.review.group,
                    e.review.field,
                    use_value.then_some(edit.as_str()),
                )
                .map_err(|e| e.to_string())?;
            e.dirty = true;
            if use_value && field == "name" {
                self.view
                    .text_input(cx, ids!(asset_name))
                    .set_text(cx, &e.asset.name);
            }
            if use_value && matches!(field.as_str(), "width" | "depth" | "height") {
                self.axis_fields(cx);
            }
            self.editor_labels(cx);
            self.suggestion_labels(cx);
            self.note(
                cx,
                &format!("{decision:?} · save the asset revision to keep this decision"),
            );
        }
        Ok(())
    }

    fn request_suggestion(&mut self, cx: &mut Cx, provider: &'static str) -> Result<(), String> {
        if self.editor.review.pending.is_some() {
            return Ok(());
        }
        let photo = self.photos.get(self.index).ok_or("Select a source photo")?;
        let marks = self
            .editor
            .asset
            .photos
            .get(&photo.id)
            .cloned()
            .unwrap_or_default();
        let scale = self.field(cx, ids!(suggest_scale));
        let reference = if scale.is_empty() {
            None
        } else {
            let length = scale
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0.)
                .ok_or("Enter a positive reference length in metres")?;
            let ends = &marks.rulers[self.editor.axis];
            if ends.len() != 2 || ends[0] == ends[1] {
                return Err(
                    "Draw the two endpoints of the coplanar reference on Photo marks".into(),
                );
            }
            Some(json!({"axis":self.editor.axis,"length_m":length,"endpoints":ends}))
        };
        let input=serde_json::to_vec(&json!({"photo":photo.id,"marks":marks,"description":self.field(cx,ids!(suggest_instruction)),
            "name":self.field(cx,ids!(asset_name)),"flat_reference":reference})).map_err(|e|e.to_string())?;
        let root = runtime::root();
        let mut cmd =
            dorobot_studio_core::api::aisle::python_command(&root).map_err(|e| e.to_string())?;
        cmd.arg(root.join("tools/aisle_workbench/asset_suggestions.py"))
            .arg(
                runtime::user_data()
                    .join("aisle/survey/photos")
                    .join(&photo.id),
            )
            .arg(provider)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (tx, rx) = mpsc::channel();
        self.editor.review.pending = Some(rx);
        self.suggestion_labels(cx);
        self.note(cx, "Reviewing the selected source photo…");
        std::thread::spawn(move || {
            let work = (|| -> Result<Suggestion, String> {
                let mut child = cmd.spawn().map_err(|e| e.to_string())?;
                let wrote = child
                    .stdin
                    .take()
                    .ok_or("Missing suggestion input pipe")?
                    .write_all(&input);
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                if !out.status.success() {
                    return Err(String::from_utf8_lossy(&out.stderr)
                        .lines()
                        .last()
                        .unwrap_or("Suggestion failed")
                        .to_owned());
                }
                wrote.map_err(|e| e.to_string())?;
                serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
            })();
            let _ = tx.send(work);
            SignalToUI::set_ui_signal();
        });
        Ok(())
    }

    pub(super) fn poll_suggestions(&mut self, cx: &mut Cx) {
        let Some(rx) = &self.editor.review.pending else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(v) => v,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("Suggestion worker stopped".into()),
        };
        self.editor.review.pending = None;
        let result = result.and_then(|s| {
            let photo = self
                .photos
                .iter()
                .find(|p| p.id == s.photo)
                .ok_or("Source photo is missing")?;
            s.validate(photo).map_err(|e| e.to_string())?;
            self.editor
                .asset
                .photos
                .entry(photo.id.clone())
                .or_default();
            self.editor.review.group = self.editor.asset.suggestions.len();
            self.editor.review.field = 0;
            self.editor.asset.suggestions.push(s);
            self.editor.dirty = true;
            Ok(())
        });
        match result {
            Ok(()) => {
                if self.editor.review.open {
                    self.open_suggestion_photo(cx);
                }
                self.note(cx, "Proposal ready for review");
            }
            Err(e) => self.note(cx, &e),
        }
        self.suggestion_labels(cx);
    }
}
