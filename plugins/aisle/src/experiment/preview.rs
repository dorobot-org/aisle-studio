use super::*;
use dorobot_studio_core::{
    survey::scene::{
        viewport::{Camera, Viewport, HEIGHT, WIDTH},
        Revision, Scene,
    },
    widgets::video_player::VideoPlayerWidgetRefExt,
};
use std::{io::Write, sync::mpsc};

struct RenderedLayout {
    rgb: Vec<u8>,
    heading: String,
    values: String,
}

#[derive(Default)]
pub(super) struct LayoutPreview {
    pub(super) repetition: usize,
    pending: Option<mpsc::Receiver<Result<RenderedLayout, String>>>,
}

impl ExperimentPane {
    fn show_preview(&self, cx: &mut Cx, open: bool) {
        self.view
            .widget(cx, ids!(exp_batch_history))
            .set_visible(cx, !open);
        self.view
            .widget(cx, ids!(exp_preview_panel))
            .set_visible(cx, open);
        self.view.label(cx, ids!(exp_history_title)).set_text(
            cx,
            if open {
                "Sampled layout"
            } else {
                "Batch history"
            },
        );
        self.view.button(cx, ids!(exp_preview)).set_text(
            cx,
            if open {
                "Back to history"
            } else {
                "Preview layout"
            },
        );
    }

    pub(super) fn close_preview(&mut self, cx: &mut Cx) {
        self.preview = LayoutPreview::default();
        self.show_preview(cx, false);
    }

    pub(super) fn request_preview(&mut self, cx: &mut Cx, repetition: usize) -> Result<(), String> {
        let cfg = self.config_input(cx)?;
        let count = cfg["repeat_count"]
            .as_u64()
            .ok_or("Enter the repeat count")? as usize;
        if !(1..=count).contains(&repetition) {
            return Err("Choose a repetition in this experiment".into());
        }
        let task = self
            .task
            .as_ref()
            .ok_or("Prepare a scene-bound controller")?;
        let assets = task
            .assets
            .iter()
            .map(|a| {
                (
                    Revision {
                        id: a.id.clone(),
                        revision: a.revision,
                    },
                    a.clone(),
                )
            })
            .collect();
        let mut command = self.command()?;
        command
            .arg("preview")
            .arg(self.controller.as_ref().ok_or("Select a controller")?)
            .arg(repetition.to_string())
            .arg("--controller-sha256")
            .arg(&self.controller_sha)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (tx, rx) = mpsc::channel();
        self.preview = LayoutPreview {
            repetition,
            pending: Some(rx),
        };
        self.show_preview(cx, true);
        self.view
            .button(cx, ids!(exp_preview_prev))
            .set_enabled(cx, repetition > 1);
        self.view
            .button(cx, ids!(exp_preview_next))
            .set_enabled(cx, repetition < count);
        self.view
            .label(cx, ids!(exp_preview_status))
            .set_text(cx, &format!("Checking repetition {repetition} of {count}…"));
        self.view
            .label(cx, ids!(exp_preview_values))
            .set_text(cx, "");
        VideoPlayerWidgetRefExt::as_video_player(&self.view.widget(cx, ids!(exp_preview_image)))
            .clear(cx);
        std::thread::spawn(move || {
            let result = (|| -> Result<RenderedLayout, String> {
                let mut child = command.spawn().map_err(|e| e.to_string())?;
                let input = serde_json::to_vec(&cfg).map_err(|e| e.to_string())?;
                let write = child.stdin.take().unwrap().write_all(&input);
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                if !output.status.success() {
                    return Err(String::from_utf8_lossy(&output.stderr)
                        .lines()
                        .last()
                        .unwrap_or("Layout unavailable")
                        .to_string());
                }
                write.map_err(|e| e.to_string())?;
                let result: Value =
                    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
                let scene: Scene =
                    serde_json::from_value(result["scene"].clone()).map_err(|e| e.to_string())?;
                let hash = result["sample_sha256"]
                    .as_str()
                    .ok_or("Missing layout digest")?;
                let seed = result["seed"].as_u64().ok_or("Missing layout seed")?;
                let view = Viewport::render(&scene, &assets, Camera::new(scene.room_m, true), None);
                let mut values = vec![format!(
                    "{} objects · {} × {} m",
                    scene.instances.len(),
                    scene.room_m[0],
                    scene.room_m[1]
                )];
                for (name, sample) in result["values"]
                    .as_object()
                    .ok_or("Missing sampled values")?
                {
                    let value = sample["value"]
                        .as_f64()
                        .map(|n| format!("{n:.4}"))
                        .unwrap_or_else(|| sample["value"].to_string());
                    values.push(
                        format!("{name} {value} {}", sample["unit"].as_str().unwrap_or(""))
                            .trim_end()
                            .to_owned(),
                    );
                }
                Ok(RenderedLayout {
                    rgb: view.image.into_raw(),
                    heading: format!(
                        "Repetition {repetition} of {count} · Seed {seed} · {}",
                        hash.get(..16).ok_or("Invalid layout digest")?
                    ),
                    values: values.join("  ·  "),
                })
            })();
            let _ = tx.send(result);
            SignalToUI::set_ui_signal();
        });
        Ok(())
    }

    pub(super) fn poll_preview(&mut self, cx: &mut Cx) {
        let Some(rx) = &self.preview.pending else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(item) => item,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("Layout preview worker stopped".into()),
        };
        self.preview.pending = None;
        match result {
            Ok(layout) => {
                self.view
                    .label(cx, ids!(exp_preview_status))
                    .set_text(cx, &layout.heading);
                self.view
                    .label(cx, ids!(exp_preview_values))
                    .set_text(cx, &layout.values);
                VideoPlayerWidgetRefExt::as_video_player(
                    &self.view.widget(cx, ids!(exp_preview_image)),
                )
                .set_frame(cx, WIDTH as usize, HEIGHT as usize, &layout.rgb);
            }
            Err(reason) => self.view.label(cx, ids!(exp_preview_status)).set_text(
                cx,
                &format!(
                    "Repetition {} unavailable · {reason}",
                    self.preview.repetition
                ),
            ),
        }
    }
}
