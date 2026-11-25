//! Optional recorded rigid-body poses accompanying a LeRobot episode.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SceneObject {
    pub name: String,
    pub size: [f32; 3],
    pub color: [f32; 4],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BasePose {
    pub position: [f32; 3],
    pub quat_xyzw: [f32; 4],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SceneFrame {
    pub timestamp: f64,
    pub sim_time_ns: i64,
    /// Column-major transforms in the same Z-up world as the URDF.
    pub transforms: Vec<[f32; 16]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_pose: Option<BasePose>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SceneReplay {
    pub version: u32,
    pub objects: Vec<SceneObject>,
    pub frames: Vec<SceneFrame>,
}

impl SceneReplay {
    pub fn validate(&self, timestamps: &[f64]) -> bool {
        (self.version == 1 || self.version == 2)
            && !self.objects.is_empty()
            && self
                .objects
                .iter()
                .map(|o| &o.name)
                .collect::<BTreeSet<_>>()
                .len()
                == self.objects.len()
            && self.objects.iter().all(|o| {
                !o.name.is_empty()
                    && o.size.iter().all(|v| v.is_finite() && *v > 0.0)
                    && o.color
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            })
            && self.frames.len() == timestamps.len()
            && self
                .frames
                .windows(2)
                .all(|f| f[0].sim_time_ns < f[1].sim_time_ns)
            && self.frames.iter().zip(timestamps).all(|(f, t)| {
                (self.version != 2 || f.base_pose.is_some())
                    && f.base_pose.as_ref().is_none_or(|p| {
                        p.position.iter().chain(&p.quat_xyzw).all(|v| v.is_finite())
                            && (p.quat_xyzw.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 0.01
                    })
                    && f.timestamp.is_finite()
                    && (f.timestamp - t).abs() < 1e-6
                    && f.transforms.len() == self.objects.len()
                    && f.transforms.iter().all(|m| {
                        m.iter().all(|v| v.is_finite())
                            && [3, 7, 11].iter().all(|i| m[*i].abs() < 1e-6)
                            && (m[15] - 1.0).abs() < 1e-6
                    })
            })
    }

    pub fn load(root: &Path, episode: u64, timestamps: &[f64]) -> Result<Option<Self>, String> {
        let path = root.join(format!("meta/scene/episode_{episode:06}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let scene: Self = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if !scene.validate(timestamps) {
            return Err("recorded scene disagrees with episode timing or geometry".into());
        }
        Ok(Some(scene))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> SceneReplay {
        serde_json::from_value(serde_json::json!({"version":1,
            "objects":[{"name":"cube", "size":[0.05,0.05,0.05],"color":[1,0,0,1]}],
            "frames":[{"timestamp":0.0,"sim_time_ns":20,
                "transforms":[[1,0,0,0, 0,1,0,0, 0,0,1,0, 0.4,0,0.2,1]]}]}))
        .unwrap()
    }
    #[test]
    fn mobile_replay_requires_a_valid_base_pose_on_every_frame() {
        let mut s = scene();
        s.version = 2;
        assert!(!s.validate(&[0.]));
        s.frames[0].base_pose = Some(BasePose {
            position: [1., 2., 0.],
            quat_xyzw: [0., 0., 0., 1.],
        });
        assert!(s.validate(&[0.]));
        s.frames[0].base_pose.as_mut().unwrap().quat_xyzw = [0.; 4];
        assert!(!s.validate(&[0.]));
    }
    #[test]
    fn scene_must_match_episode_clock_and_object_count() {
        let mut s = scene();
        assert!(s.validate(&[0.0]));
        assert!(!s.validate(&[0.1]));
        assert!(!s.validate(&[0.0, 0.1]));
        s.frames[0].transforms.clear();
        assert!(!s.validate(&[0.0]));
    }
    #[test]
    fn malformed_geometry_and_duplicate_objects_are_rejected() {
        let mut s = scene();
        s.objects[0].size[0] = -1.0;
        assert!(!s.validate(&[0.0]));
        let mut s = scene();
        s.frames[0].transforms[0][12] = f32::NAN;
        assert!(!s.validate(&[0.0]));
        let mut s = scene();
        s.objects.push(s.objects[0].clone());
        assert!(!s.validate(&[0.0]));
    }
}
