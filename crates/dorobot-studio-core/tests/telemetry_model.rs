use dorobot_studio_core::{
    api::telemetry::{JointUnit, JointValue},
    data::telemetry_model::{Axis, TelemetryModel},
};
use std::path::Path;

const MODEL: &str = "so101-follower-new-calib";

#[test]
fn so101_asset_loads_all_six_joints() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let model = TelemetryModel::resolve(MODEL, &root).unwrap();
    let robot =
        makepad_urdf_player::robot::load_robot(&model.urdf, &model.assets.to_string_lossy())
            .unwrap();
    assert_eq!(robot.num_movable_joints(), 6);
    assert!(robot.num_visual_links() >= 7);
}

#[test]
fn permutes_by_name_and_converts_units() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let model = TelemetryModel::resolve(MODEL, &root).unwrap();
    let mut joints = vec![
        joint("shoulder_pan", 90.0, JointUnit::Deg),
        joint("gripper", 100.0, JointUnit::PercentOpen),
    ];
    for name in ["elbow_flex", "shoulder_lift", "wrist_flex", "wrist_roll"] {
        joints.push(joint(name, 0.0, JointUnit::Deg));
    }
    let order = [
        "gripper",
        "wrist_roll",
        "wrist_flex",
        "elbow_flex",
        "shoulder_lift",
        "shoulder_pan",
    ]
    .map(String::from);
    let result = model.viewer_angles(&joints, &order).unwrap();
    assert!((result[0] - 1.74533).abs() < 1e-5);
    assert!((result[5] - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    joints.retain(|joint| joint.name != "elbow_flex");
    assert!(model.viewer_angles(&joints, &order).is_err());
}

fn joint(name: &str, position: f64, unit: JointUnit) -> JointValue {
    JointValue {
        name: name.into(),
        position,
        unit,
    }
}

#[test]
fn generic_units_support_prismatic_joint() {
    let model = TelemetryModel {
        urdf: Default::default(),
        assets: Default::default(),
        axes: &[("slide", Axis::Linear), ("bend", Axis::Angular)],
    };
    let values = [
        joint("bend", 1.25, JointUnit::Rad),
        joint("slide", 0.2, JointUnit::M),
    ];
    assert_eq!(
        model
            .viewer_angles(&values, &["slide".into(), "bend".into()])
            .unwrap(),
        [0.2, 1.25]
    );
}

#[test]
fn refuses_a_pose_with_extra_joints_for_an_installed_profile() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let model = TelemetryModel::resolve(MODEL, &root).unwrap();
    let order = model
        .axes
        .iter()
        .map(|(name, _)| (*name).into())
        .collect::<Vec<String>>();
    let mut values = model
        .axes
        .iter()
        .map(|(name, axis)| {
            joint(
                name,
                0.0,
                if matches!(axis, Axis::Opening { .. }) {
                    JointUnit::PercentOpen
                } else {
                    JointUnit::Deg
                },
            )
        })
        .collect::<Vec<_>>();
    values.push(joint("extra_joint", 0.0, JointUnit::Deg));
    assert!(model.viewer_angles(&values, &order).is_err());
}
