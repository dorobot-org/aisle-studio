use crate::{
    api::nexus::NexusState,
    data::{
        robot_model::RobotModelLoader,
        telemetry_model::{TelemetryModel, SO101_MODEL_ID},
    },
    widgets::video_player::VideoPlayerWidgetRefExt,
};
use makepad_urdf_player::robot_view::RobotViewWidgetRefExt;
use makepad_widgets::*;
use std::path::PathBuf;

use super::frame::{self, Themed};

const JOINTS: [&str; 6] = [
    "shoulder_pan",
    "shoulder_lift",
    "elbow_flex",
    "wrist_flex",
    "wrist_roll",
    "gripper",
];
const LABELS: [&str; 6] = [
    "Pan",
    "Lift",
    "Elbow",
    "Wrist flex",
    "Wrist roll",
    "Gripper",
];

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.NexusStage = View{
        width: Fill height: Fill flow: Right spacing: 12.0
        front := mod.widgets.ux.Card{
            width: Fill height: Fill flow: Down
            head := mod.widgets.ux.PanelHead{title +: {text: "Front camera · Nexus"}}
            camera := VideoPlayer{width: Fill height: Fill}
        }
        follower := mod.widgets.ux.Card{
            width: 310 height: Fill flow: Down spacing: 4.0
            head := mod.widgets.ux.PanelHead{title +: {text: "SO-101 · simulated follower"}}
            robot := RobotView{width: Fill height: Fill keyboard_controls: false}
            Label{width: Fill text: "Measured / target" draw_text.color: mod.theme.color_text_disabled}
            joint0 := Label{width: Fill text: ""}
            joint1 := Label{width: Fill text: ""}
            joint2 := Label{width: Fill text: ""}
            joint3 := Label{width: Fill text: ""}
            joint4 := Label{width: Fill text: ""}
            joint5 := Label{width: Fill text: ""}
        }
    }
}

#[derive(Default)]
pub struct NexusView {
    light: Option<f64>,
    model: RobotModelLoader,
    model_loaded: bool,
    applied: Option<[f32; 6]>,
    session: Option<PathBuf>,
    sequence: Option<u64>,
    replaying: bool,
}

impl NexusView {
    pub fn sync(&mut self, cx: &mut Cx, stage: &WidgetRef, state: &NexusState, replaying: bool) {
        let light = frame::light_mode();
        if self.light != Some(light) {
            frame::apply_light(
                cx,
                stage,
                &[(ids!(front), Themed::Bg), (ids!(follower), Themed::Bg)],
                light,
            );
            for path in [ids!(front.head), ids!(follower.head)] {
                frame::theme_panel_head(cx, &stage.widget(cx, path), light);
            }
            self.light = Some(light);
        }
        let camera = stage.video_player(cx, ids!(camera));
        if self.replaying != replaying {
            let title = if replaying {
                "SO-101 · final pose"
            } else {
                "SO-101 · simulated follower"
            };
            stage
                .label(cx, ids!(follower.head.title))
                .set_text(cx, title);
            self.replaying = replaying;
        }
        if self.session != state.job_dir {
            self.session.clone_from(&state.job_dir);
            camera.clear(cx);
            self.sequence = None;
            self.applied = None;
        }
        if let Some(preview) = &state.preview {
            if !replaying && self.sequence != Some(preview.sequence) {
                camera.set_frame(
                    cx,
                    usize::from(preview.width),
                    usize::from(preview.height),
                    &preview.rgb,
                );
                self.sequence = Some(preview.sequence);
            }
        }

        let viewer = stage.robot_view(cx, ids!(robot));
        if !self.model_loaded && !self.model.loading() {
            if let Some(profile) = TelemetryModel::resolve(SO101_MODEL_ID, &crate::runtime::root())
            {
                self.model.request(Some((profile.urdf, profile.assets)));
            }
        }
        if let Some(Ok(model)) = self.model.take_ready() {
            if let Some(mut viewer) = viewer.borrow_mut() {
                viewer.set_robot(cx, model);
                self.model_loaded = true;
            }
        }
        if let Some(positions) = state.snapshot.positions {
            if self.model_loaded && self.applied != Some(positions) {
                if let Some(mut viewer) = viewer.borrow_mut() {
                    let angles: Option<Vec<f32>> = viewer
                        .movable_joint_names()
                        .iter()
                        .map(|name| {
                            JOINTS
                                .iter()
                                .position(|joint| *joint == name)
                                .map(|i| positions[i])
                        })
                        .collect();
                    if let Some(angles) = angles {
                        viewer.set_joint_angles(cx, &angles);
                        self.applied = Some(positions);
                    }
                }
            }
        }
        let paths = [
            ids!(joint0),
            ids!(joint1),
            ids!(joint2),
            ids!(joint3),
            ids!(joint4),
            ids!(joint5),
        ];
        for (index, path) in paths.iter().enumerate() {
            let value = |angle: f32| {
                if index == 5 {
                    format!(
                        "{:.0}%",
                        ((angle + 0.174_533) / 1.919_863 * 100.).clamp(0., 100.)
                    )
                } else {
                    format!("{:+.1}°", angle.to_degrees())
                }
            };
            let measured = state
                .snapshot
                .positions
                .map(|q| value(q[index]))
                .unwrap_or_else(|| "—".into());
            let target = state
                .snapshot
                .targets
                .map(|q| value(q[index]))
                .unwrap_or_else(|| "—".into());
            stage.label(cx, *path).set_text(
                cx,
                &format!("{}   {} / {}", LABELS[index], measured, target),
            );
        }
    }
}
