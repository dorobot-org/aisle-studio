//! Managed LeRobot worker. Camera/motor I/O never runs on the UI thread.
use crate::data::lerobot_writer::{DatasetSpec, DatasetWriter, FrameRecord};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Rig {
    pub robot_type: String,
    pub dataset: PathBuf,
    pub task: String,
    pub fps: f64,
    pub cameras: std::collections::BTreeMap<String, Value>,
    #[serde(default = "target")]
    pub episode_target: u32,
}
fn target() -> u32 {
    50
}
impl Rig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut rig: Self = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if !["so100", "so101"].contains(&rig.robot_type.as_str())
            || !rig.fps.is_finite()
            || !(1.0..=60.0).contains(&rig.fps)
            || rig.task.trim().is_empty()
            || rig.episode_target == 0
        {
            return Err(
                "Invalid rig: choose SO-100/101, a task, 1–60 fps and a positive episode target"
                    .into(),
            );
        }
        if rig.dataset.is_relative() {
            rig.dataset = path.parent().unwrap_or(Path::new(".")).join(&rig.dataset);
        }
        Ok(rig)
    }
    pub fn spec(&self) -> DatasetSpec {
        DatasetSpec {
            robot_type: self.robot_type.clone(),
            fps: self.fps,
            camera_keys: self
                .cameras
                .keys()
                .map(|k| format!("observation.images.{k}"))
                .collect(),
            joint_names: [
                "shoulder_pan.pos",
                "shoulder_lift.pos",
                "elbow_flex.pos",
                "wrist_flex.pos",
                "wrist_roll.pos",
                "gripper.pos",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        }
    }
}

pub struct Worker {
    child: Option<Child>,
    input: Option<ChildStdin>,
    pub events: Receiver<Value>,
    pub staging: PathBuf,
    pub calibration: bool,
}
impl Worker {
    pub fn start(profile: &Path, calibration: bool) -> Result<Self, String> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let staging =
            std::env::temp_dir().join(format!("dorobot-capture-{}-{nonce}", std::process::id()));
        let mut child = Command::new(crate::runtime::native())
            .arg("capture")
            .arg(if calibration { "calibrate" } else { "record" })
            .arg(profile)
            .arg(&staging)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                format!(
                    "Could not start the native capture runtime: {e}. Check the Studio installation."
                )
            })?;
        let input = child.stdin.take();
        let stdout = child.stdout.take().ok_or("worker has no stdout")?;
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str(&line) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
            let _ = tx.send(serde_json::json!({"event":"worker_exit"}));
        });
        Ok(Self {
            child: Some(child),
            input,
            events,
            staging,
            calibration,
        })
    }
    pub fn send(&mut self, cmd: &str) -> Result<(), String> {
        let input = self.input.as_mut().ok_or("worker input is closed")?;
        writeln!(input, "{cmd}")
            .and_then(|_| input.flush())
            .map_err(|e| e.to_string())
    }
    pub fn finished(&mut self) -> bool {
        self.child
            .as_mut()
            .and_then(|c| c.try_wait().ok().flatten())
            .is_some()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.input.take(); // EOF requests a graceful disconnect before any fallback kill.
        let Some(child) = self.child.take() else {
            return;
        };
        std::thread::spawn(move || {
            let mut child = child;
            for _ in 0..50 {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            let _ = child.kill();
            let _ = child.wait();
        });
    }
}

/// Import a completed take on a background thread. All configured cameras must
/// have produced a file; the source take is retained when saving fails.
pub fn save_take(rig: &Rig, take: &Value) -> Result<u64, String> {
    let mut writer = if rig.dataset.join("meta/info.json").is_file() {
        DatasetWriter::resume(&rig.dataset, rig.spec())
    } else {
        DatasetWriter::create(&rig.dataset, rig.spec())
    }
    .map_err(|e| e.to_string())?;
    let mut ep = writer.begin_episode(&rig.task).map_err(|e| e.to_string())?;
    let path = take["frames"].as_str().ok_or("take has no frame file")?;
    for line in BufReader::new(fs::File::open(path).map_err(|e| e.to_string())?).lines() {
        let f: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let state: Vec<f32> =
            serde_json::from_value(f["state"].clone()).map_err(|e| e.to_string())?;
        let action: Vec<f32> =
            serde_json::from_value(f["action"].clone()).map_err(|e| e.to_string())?;
        ep.push_at(
            FrameRecord { state, action },
            Some(
                f["timestamp"]
                    .as_f64()
                    .ok_or("frame has no numeric timestamp")?,
            ),
        )
        .map_err(|e| e.to_string())?;
    }
    if take["count"].as_u64() != Some(ep.frame_count() as u64) {
        return Err("take frame count does not match its frame file".into());
    }
    for key in rig.spec().camera_keys {
        let source = take["videos"][&key]
            .as_str()
            .ok_or_else(|| format!("camera {key} produced no video"))?;
        fs::copy(source, ep.video_staging_path(&key)).map_err(|e| e.to_string())?;
    }
    if let Some(stats) = take.get("camera_stats") {
        ep.set_camera_stats(serde_json::from_value(stats.clone()).map_err(|e| e.to_string())?);
    }
    writer.commit(ep).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_take_saves_and_invalid_take_preserves_existing_data() {
        let root = std::env::temp_dir().join(format!("dorobot-take-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let rig = Rig {
            robot_type: "so100".into(),
            dataset: root.join("dataset"),
            task: "pick".into(),
            fps: 30.0,
            cameras: Default::default(),
            episode_target: 10,
        };
        let frames = root.join("frames.jsonl");
        let row = |t: f64| serde_json::json!({"timestamp":t,"state":[1.,2.,3.,4.,5.,6.],"action":[2.,3.,4.,5.,6.,7.]});
        fs::write(&frames, format!("{}\n{}\n", row(0.01), row(0.045))).unwrap();
        let mut take = serde_json::json!({"frames":frames,"count":2,"videos":{}});
        assert_eq!(save_take(&rig, &take).unwrap(), 0);
        let ds = crate::data::LeRobotDataset::open(&rig.dataset).unwrap();
        assert_eq!(ds.load_episode(0).unwrap().frames[1].timestamp, 0.045);
        take["count"] = serde_json::json!(1);
        assert!(save_take(&rig, &take).is_err());
        assert_eq!(
            crate::data::LeRobotDataset::open(&rig.dataset)
                .unwrap()
                .info
                .total_episodes,
            1
        );
        assert!(frames.is_file());
        fs::remove_dir_all(root).unwrap();
    }
}

/// Create an editable local rig profile without requiring a hand-written JSON file.
pub fn create_profile(robot_type: &str, follower: &str, leader: &str, cameras: &str, task: &str, destination: &str) -> Result<PathBuf, String> {
    if !matches!(robot_type.trim(), "so100" | "so101") { return Err("Robot type must be so100 or so101.".into()); }
    if follower.trim().is_empty() || leader.trim().is_empty() || follower.trim() == leader.trim() { return Err("Enter different follower and leader serial ports.".into()); }
    if task.trim().is_empty() { return Err("Describe the task to record.".into()); }
    let dataset = PathBuf::from(destination.trim());
    if !dataset.is_absolute() { return Err("Enter an absolute dataset folder.".into()); }
    let mut camera_map = serde_json::Map::new();
    for (i, id) in cameras.split(',').map(str::trim).filter(|s|!s.is_empty()).enumerate() {
        let value = if let Ok(index) = id.parse::<u32>() { serde_json::json!(index) } else if id.starts_with('/') { serde_json::json!(id) } else { return Err("Use camera numbers (0,1) or device paths, separated by commas.".into()); };
        if camera_map.values().any(|v|v == &value) { return Err("Each camera must use a different device.".into()); }
        camera_map.insert(format!("camera_{}",i+1), value);
    }
    if camera_map.is_empty() { return Err("Add at least one camera, for example 0.".into()); }
    let root = crate::preferences::path().and_then(|p|p.parent().map(PathBuf::from)).unwrap_or_else(std::env::temp_dir).join("rigs");
    std::fs::create_dir_all(&root).map_err(|e|e.to_string())?;
    let identity = |role: &str, port: &str| { use std::hash::{Hash,Hasher}; let mut h = std::collections::hash_map::DefaultHasher::new(); port.trim().hash(&mut h); format!("studio-{role}-{:x}",h.finish()) };
    let profile = serde_json::json!({"robot_type":robot_type.trim(),"follower_port":follower.trim(),"leader_port":leader.trim(),
        "follower_id":identity("follower",follower),"leader_id":identity("leader",leader),"task":task.trim(),"dataset":dataset,
        "fps":30,"cameras":camera_map,"episode_target":50,"max_relative_target":5,"max_episode_seconds":300});
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let path = root.join(format!("rig-{stamp}.json"));
    std::fs::write(&path,serde_json::to_vec_pretty(&profile).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    Ok(path)
}
