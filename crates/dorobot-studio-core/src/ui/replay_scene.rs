//! Draw recorded props alongside the articulated URDF on the same playhead.
use crate::api::scene::SceneReplay;
use makepad_urdf_player::robot_view::{ExternalItem, RobotView};
use makepad_widgets::Cx;

pub fn load(cx: &mut Cx, view: &mut RobotView) {
    let faces = [
        ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]),
        ([-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]),
        ([0., 1., 0.], [0., 0., 1.], [1., 0., 0.]),
        ([0., -1., 0.], [0., 0., -1.], [1., 0., 0.]),
        ([0., 0., 1.], [1., 0., 0.], [0., 1., 0.]),
        ([0., 0., -1.], [-1., 0., 0.], [0., 1., 0.]),
    ];
    let (mut positions, mut normals, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    for (n, u, v) in faces {
        let start = (positions.len() / 3) as u32;
        for (a, b) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
            for i in 0..3 {
                positions.push((n[i] + a * u[i] + b * v[i]) * 0.5);
            }
            normals.extend(n);
        }
        indices.extend([0, 1, 2, 0, 2, 3].map(|i| i + start));
    }
    view.upsert_external_mesh(cx, 1, &positions, &normals, &indices);
}

pub fn show(cx: &mut Cx, view: &mut RobotView, scene: &SceneReplay, frame: usize) {
    let Some(poses) = scene.frames.get(frame) else {
        return;
    };
    view.set_external_items(
        cx,
        scene
            .objects
            .iter()
            .zip(&poses.transforms)
            .map(|(object, transform)| ExternalItem {
                mesh_key: 1,
                transform: *transform,
                scale: object.size,
                color: object.color,
                roughness: 0.65,
                metallic: 0.0,
            })
            .collect(),
    );
}

/// A top-down room view with the recent, measured base trajectory. Live state
/// retains at most 1,200 frames, so this never grows with session duration.
pub fn show_trace(cx: &mut Cx, view: &mut RobotView, scene: &SceneReplay, frame: usize) {
    let Some(poses) = scene.frames.get(frame) else { return; };
    let mut items: Vec<_> = scene.objects.iter().zip(&poses.transforms)
        .map(|(object, transform)| ExternalItem {
            mesh_key: 1,
            transform: *transform,
            scale: object.size,
            color: object.color,
            roughness: 0.65,
            metallic: 0.0,
        }).collect();
    items.extend(trace_items(scene, frame));
    view.set_external_items(cx, items);
}

fn trace_items(scene: &SceneReplay, frame: usize) -> Vec<ExternalItem> {
    let mut items = Vec::new();
    let mut previous: Option<[f32; 3]> = None;
    for sample in scene.frames.iter().take(frame.saturating_add(1)) {
        let Some(base) = &sample.base_pose else { previous = None; continue; };
        let b = base.position;
        if let Some(a) = previous {
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let length = (dx * dx + dy * dy).sqrt();
            // Suppress idle solver jitter and break gaps caused by a reset.
            if length < 0.025 { continue; }
            if length <= 1.0 {
                let (c, s) = (dx / length, dy / length);
                items.push(ExternalItem {
                    mesh_key: 1,
                    transform: [c, s, 0., 0., -s, c, 0., 0., 0., 0., 1., 0.,
                        (a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, 0.04, 1.],
                    scale: [length, 0.07, 0.025],
                    color: [0.05, 0.55, 1.0, 1.0],
                    roughness: 1.0,
                    metallic: 0.0,
                });
            }
        }
        previous = Some(b);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::scene::{BasePose, SceneFrame};

    #[test]
    fn trace_uses_only_observed_motion_through_current_frame() {
        let scene = SceneReplay {
            version: 2, objects: Vec::new(),
            frames: [0.0, 0.001, 0.1, 0.2, 5.0, 5.1].into_iter().enumerate()
                .map(|(i, x)| SceneFrame {
                    timestamp: i as f64, sim_time_ns: i as i64,
                    transforms: Vec::new(),
                    base_pose: Some(BasePose {position: [x, 0., 0.], quat_xyzw: [0., 0., 0., 1.]}),
                }).collect(),
        };
        let through_second = trace_items(&scene, 2);
        assert_eq!(through_second.len(), 1);
        assert!((through_second[0].scale[0] - 0.1).abs() < 1e-6);
        assert_eq!(trace_items(&scene, 5).len(), 3); // no idle jitter / teleport line
    }
}
