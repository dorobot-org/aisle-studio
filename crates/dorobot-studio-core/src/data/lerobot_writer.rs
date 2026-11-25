//! LeRobot dataset writer — the keystone the recording console needs.
//!
//! Writes the v2.x on-disk layout that [`crate::data::LeRobotDataset`] reads,
//! so a recorded episode can be reopened by the player without a conversion
//! step. Video encoding is deliberately *not* here: frames are handed to an
//! encoder node and only their path is recorded, which keeps this module
//! testable without FFmpeg or hardware.
//!
//! ```text
//! <root>/
//!   meta/info.json          fps, robot_type, total_* counters, features
//!   meta/episodes.jsonl     one line per episode: index, length, tasks
//!   meta/tasks.jsonl        task_index -> description
//!   data/chunk-000/episode_000000.parquet
//!   videos/chunk-000/<camera key>/episode_000000.mp4
//! ```
//!
//! ## Staging
//!
//! An episode is written to a staging directory first and only moved into the
//! dataset on [`EpisodeWriter::commit`]. That is what makes the console's
//! review-before-save step cheap: discarding is a directory delete, and a
//! crashed session can never leave a half-episode in the dataset.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::builder::{FixedSizeListBuilder, Float32Builder};
use arrow_array::{FixedSizeListArray, Float64Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;

use super::DatasetError;

/// Chunk size used for `chunk-NNN` directory bucketing.
const CHUNK_SIZE: u64 = 1000;

fn chunk_dir(episode_index: u64) -> String {
    format!("chunk-{:03}", episode_index / CHUNK_SIZE)
}

fn episode_stem(episode_index: u64) -> String {
    format!("episode_{:06}", episode_index)
}

/// One recorded frame. Mirrors [`crate::data::EpisodeFrame`] on the read side.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRecord {
    pub state: Vec<f32>,
    pub action: Vec<f32>,
}

/// Describes the rig being recorded, used to build `meta/info.json`.
#[derive(Clone, Debug)]
pub struct DatasetSpec {
    pub robot_type: String,
    pub fps: f64,
    /// Camera keys in the order they appear, e.g. `observation.images.cam_high`.
    pub camera_keys: Vec<String>,
    /// Per-joint names, written into `features` so plots can label series
    /// instead of falling back to `state[0..n]`.
    pub joint_names: Vec<String>,
}

impl DatasetSpec {
    fn state_dim(&self) -> usize {
        self.joint_names.len()
    }
}

/// Creates and appends to a LeRobot dataset on disk.
pub struct DatasetWriter {
    root: PathBuf,
    _lock: File,
    spec: DatasetSpec,
    episodes: Vec<EpisodeEntry>,
    tasks: Vec<String>,
}

#[derive(Clone, Debug)]
struct EpisodeEntry {
    index: u64,
    length: u64,
    task_index: usize,
    stats: serde_json::Value,
}

impl DatasetWriter {
    /// Create a new dataset. Fails if `root` already contains one, so a
    /// mistyped path can never silently merge into an existing dataset.
    pub fn create(root: impl AsRef<Path>, spec: DatasetSpec) -> Result<Self, DatasetError> {
        let root = root.as_ref().to_path_buf();
        if root.join("meta").join("info.json").exists() {
            return Err(DatasetError::InvalidFormat(format!(
                "a dataset already exists at {}",
                root.display()
            )));
        }
        if !spec.fps.is_finite()
            || spec.fps <= 0.0
            || spec.joint_names.is_empty()
            || spec.camera_keys.iter().any(|k| {
                k.is_empty() || k.contains('/') || k.contains('\\') || k == "." || k == ".."
            })
        {
            return Err(DatasetError::InvalidFormat(
                "invalid fps, joint names, or camera key".into(),
            ));
        }
        fs::create_dir_all(root.join("meta"))?;
        let lock = lock_dataset(&root)?;
        if root.join("meta/info.json").is_file() {
            return Err(DatasetError::InvalidFormat("dataset already exists".into()));
        }
        let w = Self {
            root,
            _lock: lock,
            spec,
            episodes: Vec::new(),
            tasks: Vec::new(),
        };
        w.write_meta()?;
        Ok(w)
    }

    /// Resume a compatible v2 dataset without truncating existing recordings.
    pub fn resume(root: impl AsRef<Path>, spec: DatasetSpec) -> Result<Self, DatasetError> {
        let lock = lock_dataset(root.as_ref())?;
        recover_commit(root.as_ref())?;
        let ds = super::LeRobotDataset::open(&root)?;
        if !ds.has_per_episode_videos()
            || ds.info.robot_type != spec.robot_type
            || ds.info.fps != spec.fps
            || ds
                .info
                .features
                .get("observation.state")
                .and_then(|f| f.names.as_ref())
                != Some(&spec.joint_names)
        {
            return Err(DatasetError::InvalidFormat(
                "recording profile differs from the existing v2 dataset".into(),
            ));
        }
        let mut cameras: Vec<_> = ds
            .info
            .features
            .iter()
            .filter(|(_, v)| v.dtype == "video")
            .map(|(k, _)| k.clone())
            .collect();
        let mut expected = spec.camera_keys.clone();
        cameras.sort();
        expected.sort();
        if cameras != expected {
            return Err(DatasetError::InvalidFormat(
                "camera configuration differs".into(),
            ));
        }
        let mut tasks = ds.tasks.clone();
        tasks.sort_by_key(|t| t.task_index);
        if tasks
            .iter()
            .enumerate()
            .any(|(i, t)| t.task_index != i as u64)
        {
            return Err(DatasetError::InvalidFormat(
                "non-contiguous task table".into(),
            ));
        }
        let mut stats = BTreeMap::new();
        if let Ok(text) = fs::read_to_string(root.as_ref().join("meta/episodes_stats.jsonl")) {
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let v: serde_json::Value = serde_json::from_str(line)?;
                if let Some(i) = v["episode_index"].as_u64() {
                    stats.insert(i, v["stats"].clone());
                }
            }
        }
        let episodes = ds
            .episodes
            .iter()
            .map(|e| EpisodeEntry {
                index: e.episode_index,
                length: e.length,
                task_index: e.tasks.first().copied().unwrap_or(0) as usize,
                stats: stats
                    .remove(&e.episode_index)
                    .unwrap_or_else(|| serde_json::json!({})),
            })
            .collect();
        Ok(Self {
            root: root.as_ref().to_path_buf(),
            _lock: lock,
            spec,
            episodes,
            tasks: tasks.into_iter().map(|t| t.task).collect(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Register a task description, returning its index. Repeated descriptions
    /// map to the same index, matching LeRobot's task table semantics.
    pub fn task_index(&mut self, description: &str) -> usize {
        if let Some(i) = self.tasks.iter().position(|t| t == description) {
            return i;
        }
        self.tasks.push(description.to_string());
        self.tasks.len() - 1
    }

    /// Begin an episode. Frames are buffered in a staging directory until
    /// [`EpisodeWriter::commit`].
    pub fn begin_episode(&mut self, task: &str) -> Result<EpisodeWriter, DatasetError> {
        let index = self.next_episode_index();
        let task_index = self.task_index(task);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let staging = self
            .root
            .join(".staging")
            .join(format!("{}-{nonce}", episode_stem(index)));
        fs::create_dir_all(&staging)?;
        Ok(EpisodeWriter {
            index,
            task_index,
            staging,
            frames: Vec::new(),
            timestamps: Vec::new(),
            videos: BTreeMap::new(),
            state_dim: self.spec.state_dim(),
            camera_stats: BTreeMap::new(),
        })
    }

    fn next_episode_index(&self) -> u64 {
        self.episodes.iter().map(|e| e.index + 1).max().unwrap_or(0)
    }

    /// Move a staged episode into the dataset and refresh the metadata.
    pub fn commit(&mut self, ep: EpisodeWriter) -> Result<u64, DatasetError> {
        if ep.frames.is_empty() {
            return Err(DatasetError::InvalidFormat(
                "refusing to commit an episode with no frames".into(),
            ));
        }
        if ep.index != self.next_episode_index()
            || !ep.staging.starts_with(self.root.join(".staging"))
        {
            return Err(DatasetError::InvalidFormat(
                "staged episode is stale or belongs to another dataset".into(),
            ));
        }
        if self
            .spec
            .camera_keys
            .iter()
            .any(|k| !ep.videos.contains_key(k))
        {
            return Err(DatasetError::InvalidFormat(
                "every configured camera must produce a video".into(),
            ));
        }
        for (key, file) in &ep.videos {
            if !self.spec.camera_keys.contains(key)
                || !file.is_file()
                || file.metadata()?.len() == 0
            {
                return Err(DatasetError::InvalidFormat(format!(
                    "missing or undeclared camera video: {key}"
                )));
            }
        }
        let index = ep.index;

        // 1. frames -> data/chunk-NNN/episode_NNNNNN.parquet
        let data_dir = self.root.join("data").join(chunk_dir(index));
        fs::create_dir_all(&data_dir)?;
        let parquet_path = data_dir.join(format!("{}.parquet", episode_stem(index)));
        if parquet_path.exists() {
            return Err(DatasetError::InvalidFormat(
                "episode file already exists; refusing overwrite".into(),
            ));
        }
        let staged_parquet = ep.staging.join("frames.parquet");
        write_episode_parquet(
            &staged_parquet,
            &ep,
            self.spec.fps,
            self.episodes.iter().map(|e| e.length).sum(),
        )?;

        let mut files = vec![parquet_path.clone()];
        let mut copies = Vec::new();
        for (key, staged) in &ep.videos {
            let dst_dir = self.root.join("videos").join(chunk_dir(index)).join(key);
            fs::create_dir_all(&dst_dir)?;
            let dst = dst_dir.join(format!("{}.mp4", episode_stem(index)));
            if dst.exists() {
                return Err(DatasetError::InvalidFormat(
                    "camera file already exists; refusing overwrite".into(),
                ));
            }
            files.push(dst.clone());
            copies.push((staged, dst));
        }
        begin_commit(&self.root, &files)?;
        let previous_len = self.episodes.len();
        let result = (|| -> Result<(), DatasetError> {
            for (source, dst) in copies {
                fs::copy(source, &dst)?;
                File::open(dst)?.sync_all()?;
            }
            File::open(&staged_parquet)?.sync_all()?;
            fs::rename(staged_parquet, parquet_path)?;
            self.episodes.push(EpisodeEntry {
                index,
                length: ep.frames.len() as u64,
                task_index: ep.task_index,
                stats: episode_stats(
                    &ep,
                    self.spec.fps,
                    self.episodes.iter().map(|e| e.length).sum(),
                ),
            });
            self.write_meta()?;
            fs::remove_file(self.root.join("meta/.dorobot-commit.json"))?;
            Ok(())
        })();
        if let Err(e) = result {
            self.episodes.truncate(previous_len);
            recover_commit(&self.root)?;
            return Err(e);
        }

        let _ = fs::remove_dir_all(&ep.staging);
        Ok(index)
    }

    /// Throw away a staged episode.
    pub fn discard(&self, ep: EpisodeWriter) -> Result<(), DatasetError> {
        if ep.staging.exists() {
            fs::remove_dir_all(&ep.staging)?;
        }
        Ok(())
    }

    /// Rewrite `meta/`. Called after every commit so an interrupted session
    /// still leaves a readable dataset.
    fn write_meta(&self) -> Result<(), DatasetError> {
        let meta = self.root.join("meta");
        fs::create_dir_all(&meta)?;

        let total_frames: u64 = self.episodes.iter().map(|e| e.length).sum();
        let dim = self.spec.state_dim();
        let mut features = serde_json::json!({
            "observation.state": {"dtype":"float32", "shape":[dim], "names":self.spec.joint_names},
            "action": {"dtype":"float32", "shape":[dim], "names":self.spec.joint_names},
            "timestamp": {"dtype":"float64", "shape":[1], "names":null},
            "frame_index": {"dtype":"int64", "shape":[1], "names":null},
            "episode_index": {"dtype":"int64", "shape":[1], "names":null},
            "index": {"dtype":"int64", "shape":[1], "names":null},
            "task_index": {"dtype":"int64", "shape":[1], "names":null}
        });
        for key in &self.spec.camera_keys {
            features[key] = serde_json::json!({"dtype":"video", "shape":[480,640,3], "names":["height","width","channel"], "info":{"video.fps":self.spec.fps,"video.codec":"mpeg4","video.pix_fmt":"yuv420p","video.is_depth_map":false,"has_audio":false}});
        }
        let info = serde_json::json!({
            "codebase_version":"v2.1", "robot_type":self.spec.robot_type, "fps":self.spec.fps,
            "total_episodes":self.episodes.len(), "total_frames":total_frames,
            "total_tasks":self.tasks.len(), "total_videos":self.episodes.len()*self.spec.camera_keys.len(),
            "total_chunks":self.episodes.last().map(|e| e.index / CHUNK_SIZE + 1).unwrap_or(0),
            "chunks_size":CHUNK_SIZE, "features":features,
            "splits":{"train":format!("0:{}",self.episodes.len())},
            "data_path":"data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet",
            "video_path":"videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4"
        });
        // Publish info last, so it never advertises an episode before the index.
        let info_text = serde_json::to_vec_pretty(&info)?;

        let mut episodes_jsonl = File::create(meta.join(".episodes.jsonl.tmp"))?;
        for e in &self.episodes {
            let task = self
                .tasks
                .get(e.task_index)
                .map(String::as_str)
                .unwrap_or("");
            writeln!(
                episodes_jsonl,
                r#"{{"episode_index": {}, "tasks": [{}], "length": {}, "task": {}}}"#,
                e.index,
                serde_json::to_string(task)?,
                e.length,
                serde_json::to_string(task).unwrap_or_else(|_| "\"\"".into()),
            )?;
        }

        let mut tasks_jsonl = File::create(meta.join(".tasks.jsonl.tmp"))?;
        for (i, t) in self.tasks.iter().enumerate() {
            writeln!(
                tasks_jsonl,
                r#"{{"task_index": {}, "task": {}}}"#,
                i,
                serde_json::to_string(t).unwrap_or_else(|_| "\"\"".into())
            )?;
        }
        let mut stats_file = File::create(meta.join(".episodes_stats.jsonl.tmp"))?;
        for e in &self.episodes {
            writeln!(
                stats_file,
                "{}",
                serde_json::json!({"episode_index":e.index,"stats":e.stats})
            )?;
        }
        stats_file.sync_all()?;
        fs::rename(
            meta.join(".episodes_stats.jsonl.tmp"),
            meta.join("episodes_stats.jsonl"),
        )?;
        episodes_jsonl.sync_all()?;
        tasks_jsonl.sync_all()?;
        fs::rename(meta.join(".tasks.jsonl.tmp"), meta.join("tasks.jsonl"))?;
        fs::rename(
            meta.join(".episodes.jsonl.tmp"),
            meta.join("episodes.jsonl"),
        )?;
        fs::write(meta.join(".info.json.tmp"), info_text)?;
        fs::rename(meta.join(".info.json.tmp"), meta.join("info.json"))?;
        Ok(())
    }
}

// Journal metadata before publishing any episode bytes. Reopening a writer
// rolls an interrupted commit back; the original capture remains in staging.
const META_FILES: &[&str] = &[
    "info.json",
    "episodes.jsonl",
    "tasks.jsonl",
    "episodes_stats.jsonl",
];
#[derive(serde::Serialize, serde::Deserialize)]
struct CommitJournal {
    metadata: BTreeMap<String, Option<Vec<u8>>>,
    files: Vec<PathBuf>,
}
fn begin_commit(root: &Path, files: &[PathBuf]) -> Result<(), DatasetError> {
    let mut metadata = BTreeMap::new();
    for name in META_FILES {
        let path = root.join("meta").join(name);
        metadata.insert(
            name.to_string(),
            if path.exists() {
                Some(fs::read(path)?)
            } else {
                None
            },
        );
    }
    let journal = CommitJournal {
        metadata,
        files: files
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_path_buf())
            .collect(),
    };
    let temp = root.join("meta/.dorobot-commit.tmp");
    let mut f = File::create(&temp)?;
    f.write_all(&serde_json::to_vec(&journal)?)?;
    f.sync_all()?;
    fs::rename(temp, root.join("meta/.dorobot-commit.json"))?;
    Ok(())
}
fn recover_commit(root: &Path) -> Result<(), DatasetError> {
    let path = root.join("meta/.dorobot-commit.json");
    if !path.is_file() {
        return Ok(());
    }
    let journal: CommitJournal = serde_json::from_slice(&fs::read(&path)?)?;
    if journal
        .metadata
        .keys()
        .any(|k| !META_FILES.contains(&k.as_str()))
        || journal.files.iter().any(|p| {
            p.is_absolute()
                || p.components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
        })
    {
        return Err(DatasetError::InvalidFormat(
            "invalid commit recovery journal".into(),
        ));
    }
    for (name, bytes) in journal.metadata {
        let dst = root.join("meta").join(name);
        if let Some(bytes) = bytes {
            let temp = dst.with_extension("recovery.tmp");
            fs::write(&temp, bytes)?;
            File::open(&temp)?.sync_all()?;
            fs::rename(temp, dst)?;
        } else if dst.is_file() {
            fs::remove_file(dst)?;
        }
    }
    for file in journal.files {
        let p = root.join(file);
        if p.is_file() {
            fs::remove_file(p)?;
        }
    }
    fs::remove_file(path)?;
    Ok(())
}

fn lock_dataset(root: &Path) -> Result<File, DatasetError> {
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".dorobot-writer.lock"))?;
    file.try_lock().map_err(|e| {
        DatasetError::InvalidFormat(format!("dataset is being recorded by another writer: {e}"))
    })?;
    Ok(file)
}

/// A single episode being recorded into staging.
pub struct EpisodeWriter {
    index: u64,
    task_index: usize,
    staging: PathBuf,
    frames: Vec<FrameRecord>,
    timestamps: Vec<Option<f64>>,
    videos: BTreeMap<String, PathBuf>,
    state_dim: usize,
    camera_stats: BTreeMap<String, serde_json::Value>,
}

impl EpisodeWriter {
    pub fn set_camera_stats(&mut self, stats: BTreeMap<String, serde_json::Value>) {
        self.camera_stats = stats;
    }

    pub fn index(&self) -> u64 {
        self.index
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Where an encoder node should write this episode's video for `key`.
    pub fn video_staging_path(&mut self, key: &str) -> PathBuf {
        let p = self.staging.join(format!("{}.mp4", key.replace('/', "_")));
        self.videos.insert(key.to_string(), p.clone());
        p
    }

    /// Append one frame. Dimension mismatches are rejected at push time rather
    /// than producing a dataset that fails to load later.
    pub fn push(&mut self, frame: FrameRecord) -> Result<(), DatasetError> {
        self.push_at(frame, None)
    }

    pub fn push_at(
        &mut self,
        frame: FrameRecord,
        timestamp: Option<f64>,
    ) -> Result<(), DatasetError> {
        if frame
            .state
            .iter()
            .chain(&frame.action)
            .any(|v| !v.is_finite())
            || timestamp.is_some_and(|t| {
                !t.is_finite()
                    || t < 0.0
                    || self
                        .timestamps
                        .last()
                        .and_then(|t| *t)
                        .is_some_and(|last| t <= last)
            })
        {
            return Err(DatasetError::InvalidFormat(
                "non-finite frame or non-monotonic timestamp".into(),
            ));
        }
        if frame.state.len() != self.state_dim || frame.action.len() != self.state_dim {
            return Err(DatasetError::InvalidFormat(format!(
                "frame {} has state {} / action {}, expected {}",
                self.frames.len(),
                frame.state.len(),
                frame.action.len(),
                self.state_dim
            )));
        }
        self.frames.push(frame);
        self.timestamps.push(timestamp);
        Ok(())
    }

    /// Integrity findings surfaced to the operator before saving.
    pub fn warnings(&self, expected_cameras: usize) -> Vec<String> {
        let mut out = Vec::new();
        if self.frames.is_empty() {
            out.push("no frames captured".into());
        }
        if self.videos.len() < expected_cameras {
            out.push(format!(
                "{} of {} cameras produced video",
                self.videos.len(),
                expected_cameras
            ));
        }
        for (key, path) in &self.videos {
            if !path.exists() {
                out.push(format!("{key}: encoder produced no file"));
            }
        }
        out
    }
}

fn episode_stats(ep: &EpisodeWriter, fps: f64, start: u64) -> serde_json::Value {
    fn stats(rows: Vec<Vec<f64>>) -> serde_json::Value {
        let n = rows.len();
        let dim = rows.first().map(|r| r.len()).unwrap_or(0);
        let mut min = vec![f64::INFINITY; dim];
        let mut max = vec![f64::NEG_INFINITY; dim];
        let mut mean = vec![0.0; dim];
        let mut variance = vec![0.0; dim];
        for row in &rows {
            for i in 0..dim {
                min[i] = min[i].min(row[i]);
                max[i] = max[i].max(row[i]);
                mean[i] += row[i] / n as f64;
            }
        }
        for row in &rows {
            for i in 0..dim {
                variance[i] += (row[i] - mean[i]).powi(2) / n as f64;
            }
        }
        serde_json::json!({"min":min,"max":max,"mean":mean,"std":variance.iter().map(|v|v.sqrt()).collect::<Vec<_>>(),"count":[n]})
    }
    let n = ep.frames.len();
    let mut out = serde_json::json!({});
    out["observation.state"] = stats(
        ep.frames
            .iter()
            .map(|f| f.state.iter().map(|v| *v as f64).collect())
            .collect(),
    );
    out["action"] = stats(
        ep.frames
            .iter()
            .map(|f| f.action.iter().map(|v| *v as f64).collect())
            .collect(),
    );
    out["timestamp"] = stats(
        (0..n)
            .map(|i| vec![ep.timestamps[i].unwrap_or(i as f64 / fps)])
            .collect(),
    );
    out["frame_index"] = stats((0..n).map(|i| vec![i as f64]).collect());
    out["index"] = stats((0..n).map(|i| vec![(start + i as u64) as f64]).collect());
    out["episode_index"] = stats(vec![vec![ep.index as f64]; n]);
    out["task_index"] = stats(vec![vec![ep.task_index as f64]; n]);
    for (key, value) in &ep.camera_stats {
        out[key] = value.clone();
    }
    out
}

/// Write the frame table.
///
/// Types are dictated by the reader, not by taste: it downcasts `frame_index`
/// to `Int64` and the vector columns to `FixedSizeList<Float32>`. Writing
/// `ListArray`/`UInt64` here produces a file that opens but yields empty
/// states — which is precisely what the round-trip test caught.
fn write_episode_parquet(
    path: &Path,
    ep: &EpisodeWriter,
    fps: f64,
    start_index: u64,
) -> Result<(), DatasetError> {
    let n = ep.frames.len();
    let dim = ep.state_dim as i32;

    let vec_field = |name: &str| {
        Field::new(
            name,
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), dim),
            false,
        )
    };
    let schema = Arc::new(Schema::new(vec![
        Field::new("frame_index", DataType::Int64, false),
        Field::new("timestamp", DataType::Float64, false),
        Field::new("episode_index", DataType::Int64, false),
        vec_field("observation.state"),
        vec_field("action"),
        Field::new("index", DataType::Int64, false),
        Field::new("task_index", DataType::Int64, false),
    ]));

    let frame_index = Int64Array::from((0..n as i64).collect::<Vec<_>>());
    let timestamp = Float64Array::from(
        (0..n)
            .map(|i| ep.timestamps[i].unwrap_or(i as f64 / fps))
            .collect::<Vec<_>>(),
    );
    let episode_index = Int64Array::from(vec![ep.index as i64; n]);

    let mut state_b =
        FixedSizeListBuilder::new(Float32Builder::with_capacity(n * dim as usize), dim);
    let mut action_b =
        FixedSizeListBuilder::new(Float32Builder::with_capacity(n * dim as usize), dim);
    for f in &ep.frames {
        state_b.values().append_slice(&f.state);
        state_b.append(true);
        action_b.values().append_slice(&f.action);
        action_b.append(true);
    }
    let state: FixedSizeListArray = state_b.finish();
    let action: FixedSizeListArray = action_b.finish();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(frame_index),
            Arc::new(timestamp),
            Arc::new(episode_index),
            Arc::new(state),
            Arc::new(action),
            Arc::new(Int64Array::from(
                (start_index as i64..start_index as i64 + n as i64).collect::<Vec<_>>(),
            )),
            Arc::new(Int64Array::from(vec![ep.task_index as i64; n])),
        ],
    )
    .map_err(|e| DatasetError::InvalidFormat(format!("building record batch: {e}")))?;

    let file = File::create(path)?;
    let props = WriterProperties::builder().build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))
        .map_err(|e| DatasetError::InvalidFormat(format!("opening parquet writer: {e}")))?;
    writer
        .write(&batch)
        .map_err(|e| DatasetError::InvalidFormat(format!("writing parquet: {e}")))?;
    writer
        .close()
        .map_err(|e| DatasetError::InvalidFormat(format!("closing parquet: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::LeRobotDataset;

    fn spec() -> DatasetSpec {
        DatasetSpec {
            robot_type: "so101".into(),
            fps: 30.0,
            camera_keys: vec![],
            joint_names: vec![
                "shoulder_pan".into(),
                "shoulder_lift".into(),
                "elbow_flex".into(),
            ],
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("dorobot-writer-{name}"));
        let _ = fs::remove_dir_all(&p);
        p
    }

    /// The whole point of the writer: what it writes, the shipping loader reads.
    #[test]
    fn round_trips_through_the_reader() {
        let root = tmp("roundtrip");
        let mut w = DatasetWriter::create(&root, spec()).unwrap();

        let mut ep = w.begin_episode("Pick the block").unwrap();
        for i in 0..48 {
            let t = i as f32 * 0.1;
            ep.push(FrameRecord {
                state: vec![t.sin(), t.cos(), t * 0.01],
                action: vec![t.sin() + 0.05, t.cos() - 0.02, t * 0.011],
            })
            .unwrap();
        }
        let idx = w.commit(ep).unwrap();
        assert_eq!(idx, 0);

        let ds = LeRobotDataset::open(&root).expect("writer output must be loadable");
        assert_eq!(ds.num_episodes(), 1);
        assert_eq!(ds.info.robot_type, "so101");
        assert_eq!(ds.info.fps, 30.0);
        assert_eq!(ds.get_task(0), Some("Pick the block"));

        let data = ds.load_episode(0).expect("episode must load");
        assert_eq!(data.frames.len(), 48);
        let f0 = &data.frames[0];
        assert_eq!(f0.frame_index, 0);
        assert_eq!(f0.state.len(), 3);
        assert_eq!(f0.action.len(), 3);
        // timestamps derive from fps, so drift is zero for a clean write
        let last = data.frames.last().unwrap();
        assert!(
            (last.timestamp - 47.0 / 30.0).abs() < 1e-6,
            "ts {}",
            last.timestamp
        );
    }

    #[test]
    fn appends_multiple_episodes_and_reuses_task_indices() {
        let root = tmp("append");
        let mut w = DatasetWriter::create(&root, spec()).unwrap();
        for _ in 0..3 {
            let mut ep = w.begin_episode("same task").unwrap();
            for _ in 0..5 {
                ep.push(FrameRecord {
                    state: vec![0.0; 3],
                    action: vec![0.0; 3],
                })
                .unwrap();
            }
            w.commit(ep).unwrap();
        }
        let ds = LeRobotDataset::open(&root).unwrap();
        assert_eq!(ds.num_episodes(), 3);
        assert_eq!(ds.info.total_frames, 15);
        // one description reused across episodes -> a single task entry
        assert_eq!(ds.get_task(0), Some("same task"));
        assert_eq!(ds.get_task(1), None);
        assert_eq!(ds.load_episode(2).unwrap().frames.len(), 5);
    }

    #[test]
    fn discard_leaves_no_trace() {
        let root = tmp("discard");
        let mut w = DatasetWriter::create(&root, spec()).unwrap();
        let mut ep = w.begin_episode("throwaway").unwrap();
        ep.push(FrameRecord {
            state: vec![1.0; 3],
            action: vec![1.0; 3],
        })
        .unwrap();
        let staging = ep.staging.clone();
        w.discard(ep).unwrap();
        assert!(!staging.exists());
        assert!(
            !root.join("data").exists(),
            "discard must not touch the dataset"
        );
    }

    #[test]
    fn rejects_bad_frames_and_empty_episodes() {
        let root = tmp("reject");
        let mut w = DatasetWriter::create(&root, spec()).unwrap();
        let mut ep = w.begin_episode("t").unwrap();
        assert!(ep
            .push(FrameRecord {
                state: vec![0.0; 2],
                action: vec![0.0; 3]
            })
            .is_err());
        assert!(w.commit(ep).is_err(), "empty episode must not commit");
    }

    #[test]
    fn reports_missing_camera_video() {
        let root = tmp("warn");
        let mut w = DatasetWriter::create(&root, spec()).unwrap();
        let mut ep = w.begin_episode("t").unwrap();
        ep.push(FrameRecord {
            state: vec![0.0; 3],
            action: vec![0.0; 3],
        })
        .unwrap();
        let warnings = ep.warnings(1);
        assert!(
            warnings.iter().any(|w| w.contains("0 of 1 cameras")),
            "{warnings:?}"
        );
    }
    #[test]
    fn lock_resume_and_interrupted_commit_recovery() {
        let root = tmp("recovery");
        let mut w = DatasetWriter::create(&root, spec()).unwrap();
        assert!(DatasetWriter::resume(&root, spec()).is_err());
        let mut ep = w.begin_episode("quoted \"task\" \\ test").unwrap();
        ep.push_at(
            FrameRecord {
                state: vec![1., 2., 3.],
                action: vec![2., 3., 4.],
            },
            Some(0.01),
        )
        .unwrap();
        ep.push_at(
            FrameRecord {
                state: vec![2., 3., 4.],
                action: vec![3., 4., 5.],
            },
            Some(0.06),
        )
        .unwrap();
        w.commit(ep).unwrap();
        let stats: serde_json::Value = serde_json::from_str(
            fs::read_to_string(root.join("meta/episodes_stats.jsonl"))
                .unwrap()
                .trim(),
        )
        .unwrap();
        assert_eq!(
            stats["stats"]["observation.state"]["mean"],
            serde_json::json!([1.5, 2.5, 3.5])
        );
        let orphan = root.join("data/chunk-000/episode_000001.parquet");
        begin_commit(&root, &[orphan.clone()]).unwrap();
        fs::write(&orphan, b"incomplete").unwrap();
        fs::write(root.join("meta/info.json"), b"incomplete").unwrap();
        drop(w);
        let w = DatasetWriter::resume(&root, spec()).unwrap();
        assert_eq!(w.next_episode_index(), 1);
        assert!(!orphan.exists());
        let ds = LeRobotDataset::open(&root).unwrap();
        let data = ds.load_episode(0).unwrap();
        assert_eq!(data.frames[1].timestamp, 0.06);
        drop(w);
        fs::remove_dir_all(root).unwrap();
    }
}
