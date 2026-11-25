//! Local model profiles interpret named telemetry without involving the receiver.

use crate::api::telemetry::{JointUnit, JointValue};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const SO101_MODEL_ID: &str = "so101-follower-new-calib";

#[derive(Debug, Clone, Copy)]
pub enum Axis {
    Angular,
    Linear,
    Opening { closed: f64, open: f64 },
}

impl Axis {
    fn map(self, value: &JointValue) -> Option<f32> {
        let position = match (self, value.unit) {
            (Self::Angular, JointUnit::Rad) | (Self::Linear, JointUnit::M) => value.position,
            (Self::Angular, JointUnit::Deg) => value.position.to_radians(),
            (Self::Opening { closed, open }, JointUnit::PercentOpen)
                if (0.0..=100.0).contains(&value.position) =>
            {
                closed + (open - closed) * (value.position / 100.0)
            }
            _ => return None,
        } as f32;
        position.is_finite().then_some(position)
    }
}

const SO101_AXES: &[(&str, Axis)] = &[
    ("shoulder_pan", Axis::Angular),
    ("shoulder_lift", Axis::Angular),
    ("elbow_flex", Axis::Angular),
    ("wrist_flex", Axis::Angular),
    ("wrist_roll", Axis::Angular),
    (
        "gripper",
        Axis::Opening {
            closed: -0.174533,
            open: 1.74533,
        },
    ),
];

#[derive(Debug, Clone)]
pub struct TelemetryModel {
    pub urdf: PathBuf,
    pub assets: PathBuf,
    pub axes: &'static [(&'static str, Axis)],
}

#[derive(Debug, Error)]
pub enum PoseError {
    #[error("telemetry profile and viewer joints differ")]
    ViewerOrder,
    #[error("received joint count differs from the telemetry profile")]
    JointSet,
    #[error("telemetry is missing position for {0}")]
    Missing(String),
    #[error("joint {0} has an incompatible telemetry unit or value")]
    Unit(String),
}

impl TelemetryModel {
    pub fn resolve(model_id: &str, studio_root: &Path) -> Option<Self> {
        if model_id != SO101_MODEL_ID {
            return None;
        }
        let assets = studio_root.join("data/so101");
        Some(Self {
            urdf: assets.join("so101_new_calib.urdf"),
            assets,
            axes: SO101_AXES,
        })
    }

    pub fn viewer_angles(
        &self,
        joints: &[JointValue],
        viewer_order: &[String],
    ) -> Result<Vec<f32>, PoseError> {
        if joints.len() != self.axes.len() {
            return Err(PoseError::JointSet);
        }
        if viewer_order.len() != self.axes.len() {
            return Err(PoseError::ViewerOrder);
        }
        viewer_order
            .iter()
            .map(|name| {
                let axis = self
                    .axes
                    .iter()
                    .find(|(candidate, _)| *candidate == name)
                    .ok_or(PoseError::ViewerOrder)?
                    .1;
                let value = joints
                    .iter()
                    .find(|joint| joint.name == *name)
                    .ok_or_else(|| PoseError::Missing(name.clone()))?;
                axis.map(value).ok_or_else(|| PoseError::Unit(name.clone()))
            })
            .collect()
    }
}
