//! LeRobot Dataset Loader
//!
//! Parses LeRobot v2.0 and v3.0 dataset formats.

use std::collections::HashMap;

/// Batch size for Arrow parquet reader (rows per batch)
const ARROW_BATCH_SIZE: usize = 8192;

/// Maximum number of chunk directories to search
const MAX_CHUNK_SEARCH: u32 = 100;

/// Maximum number of files to search within a chunk
const MAX_FILE_SEARCH: u32 = 100;

/// Maximum number of video files to search within a camera directory
const MAX_VIDEO_FILE_SEARCH: u32 = 10;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Float64Array, Int64Array, ListArray, RecordBatchReader,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::RowAccessor;
use serde::{Deserialize, Serialize};

/// Read a float vector cell.
///
/// v2.0 files store `observation.state` / `action` as `FixedSizeList<float>`,
/// v3.0 files as `List<float>`. Accepting only one silently yields empty
/// states — the dataset loads, the frame count looks right, and every plot is
/// blank. Accept both.
fn float_vec_at(col: &dyn Array, row: usize) -> Vec<f32> {
    if let Some(a) = col.as_any().downcast_ref::<FixedSizeListArray>() {
        return a
            .value(row)
            .as_any()
            .downcast_ref::<Float32Array>()
            .map(|v| v.values().to_vec())
            .unwrap_or_default();
    }
    if let Some(a) = col.as_any().downcast_ref::<ListArray>() {
        return a
            .value(row)
            .as_any()
            .downcast_ref::<Float32Array>()
            .map(|v| v.values().to_vec())
            .unwrap_or_default();
    }
    Vec::new()
}

/// Timestamps are f64 in v2.0 files and f32 in v3.0 files.
fn f64_at(col: &dyn Array, row: usize) -> Option<f64> {
    if let Some(a) = col.as_any().downcast_ref::<Float64Array>() {
        return Some(a.value(row));
    }
    if let Some(a) = col.as_any().downcast_ref::<Float32Array>() {
        return Some(a.value(row) as f64);
    }
    None
}
use arrow_schema::ArrowError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatasetError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("Arrow error: {0}")]
    Arrow(#[from] ArrowError),
    #[error("Missing file: {0}")]
    MissingFile(String),
    #[error("Invalid dataset format: {0}")]
    InvalidFormat(String),
}

fn row_field<'a>(row: &'a parquet::record::Row, key: &str) -> Option<&'a parquet::record::Field> {
    row.get_column_iter()
        .find(|(k, _)| k.as_str() == key)
        .map(|(_, v)| v)
}
fn row_u64(row: &parquet::record::Row, key: &str) -> Option<u64> {
    use parquet::record::Field;
    match row_field(row, key)? {
        Field::Long(v) => (*v).try_into().ok(),
        Field::Int(v) => (*v).try_into().ok(),
        Field::ULong(v) => Some(*v),
        Field::UInt(v) => Some(*v as u64),
        _ => None,
    }
}
fn row_f64(row: &parquet::record::Row, key: &str) -> Option<f64> {
    use parquet::record::Field;
    match row_field(row, key)? {
        Field::Double(v) => Some(*v),
        Field::Float(v) => Some(*v as f64),
        _ => None,
    }
}
fn row_string(row: &parquet::record::Row, key: &str) -> Option<String> {
    match row_field(row, key)? {
        parquet::record::Field::Str(v) => Some(v.clone()),
        _ => None,
    }
}
fn intern_task(tasks: &mut Vec<TaskInfo>, text: &str) -> u64 {
    if let Some(t) = tasks.iter().find(|t| t.task == text) {
        return t.task_index;
    }
    let task_index = tasks.iter().map(|t| t.task_index + 1).max().unwrap_or(0);
    tasks.push(TaskInfo {
        task_index,
        task: text.into(),
    });
    task_index
}
fn parquet_files(root: &Path) -> Result<Vec<PathBuf>, DatasetError> {
    let mut files = Vec::new();
    if !root.exists() {
        return Ok(files);
    }
    for e in std::fs::read_dir(root)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            files.extend(parquet_files(&e.path())?);
        } else if e.path().extension().is_some_and(|s| s == "parquet") {
            files.push(e.path());
        }
    }
    files.sort();
    Ok(files)
}

/// Dataset info from meta/info.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetInfo {
    #[serde(default)]
    pub codebase_version: String,
    #[serde(default)]
    pub robot_type: String,
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub total_episodes: u64,
    #[serde(default)]
    pub total_frames: u64,
    #[serde(default)]
    pub features: HashMap<String, FeatureInfo>,
    #[serde(default)]
    pub video: bool,
    #[serde(default = "default_chunk_size")]
    pub chunks_size: u64,
}

fn default_chunk_size() -> u64 {
    1000
}

impl Default for DatasetInfo {
    fn default() -> Self {
        Self {
            codebase_version: "2.0".to_string(),
            robot_type: "unknown".to_string(),
            fps: 30.0,
            total_episodes: 0,
            total_frames: 0,
            features: HashMap::new(),
            video: false,
            chunks_size: 1000,
        }
    }
}

/// Feature info from info.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureInfo {
    #[serde(default)]
    pub dtype: String,
    #[serde(default)]
    pub shape: Vec<u64>,
    /// Names can be either an array of strings, an object with named arrays, or null
    #[serde(default, deserialize_with = "deserialize_names")]
    pub names: Option<Vec<String>>,
}

/// Deserialize names which can be:
/// - null -> None
/// - ["name1", "name2"] -> Some(vec!["name1", "name2"])
/// - { "motors": ["name1", "name2"] } -> Some(vec!["name1", "name2"])
fn deserialize_names<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, MapAccess, SeqAccess, Visitor};

    struct NamesVisitor;

    impl<'de> Visitor<'de> for NamesVisitor {
        type Value = Option<Vec<String>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("null, array of strings, or object containing array of strings")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut names = Vec::new();
            while let Some(name) = seq.next_element::<String>()? {
                names.push(name);
            }
            Ok(Some(names))
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            // Take the first array value found in the map
            while let Some((_, value)) = map.next_entry::<String, Vec<String>>()? {
                return Ok(Some(value));
            }
            Ok(None)
        }
    }

    deserializer.deserialize_any(NamesVisitor)
}

/// Task info from meta/tasks.jsonl
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub task_index: u64,
    pub task: String,
}

/// Episode metadata from meta/episodes.parquet
#[derive(Debug, Clone)]
pub struct EpisodeMetadata {
    pub episode_index: u64,
    pub tasks: Vec<u64>,
    pub length: u64,
    /// v3 files are bucketed independently of episode numbers.
    pub data_file: Option<(u64, u64)>,
    /// camera -> (chunk, file, starting timestamp in that file).
    pub video_files: HashMap<String, (u64, u64, f64)>,
}

/// Episode data frame
#[derive(Debug, Clone)]
pub struct EpisodeFrame {
    pub frame_index: u64,
    pub timestamp: f64,
    pub state: Vec<f32>,
    pub action: Vec<f32>,
}

/// Loaded episode data
#[derive(Debug, Clone)]
pub struct EpisodeData {
    pub episode_index: u64,
    pub frames: Vec<EpisodeFrame>,
    pub video_paths: HashMap<String, PathBuf>,
    /// Starting frame index in the video file (for v3.0 concatenated videos)
    pub video_frame_offset: u64,
    pub video_frame_offsets: HashMap<String, u64>,
}

/// LeRobot Dataset
pub struct LeRobotDataset {
    pub root_path: PathBuf,
    pub info: DatasetInfo,
    pub tasks: Vec<TaskInfo>,
    pub episodes: Vec<EpisodeMetadata>,
}

impl LeRobotDataset {
    /// Open a LeRobot dataset from a directory
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, DatasetError> {
        let root_path = path.as_ref().to_path_buf();

        // Load info.json first (needed for v3.0 episode loading)
        let info = Self::load_info(&root_path)?;

        // Load tasks (supports both v2.0 and v3.0)
        let mut tasks = Self::load_tasks(&root_path)?;

        // Load episodes (supports both v2.0 and v3.0)
        let episodes = Self::load_episode_metadata(&root_path, &info, &mut tasks)?;

        Ok(Self {
            root_path,
            info,
            tasks,
            episodes,
        })
    }

    /// Load meta/info.json
    fn load_info(root: &Path) -> Result<DatasetInfo, DatasetError> {
        let info_path = root.join("meta").join("info.json");
        if !info_path.exists() {
            return Err(DatasetError::MissingFile("meta/info.json".to_string()));
        }

        let file = File::open(&info_path)?;
        let reader = BufReader::new(file);
        let info: DatasetInfo = serde_json::from_reader(reader)?;
        if !info.fps.is_finite() || info.fps <= 0.0 || info.chunks_size == 0 {
            return Err(DatasetError::InvalidFormat(
                "fps and chunks_size must be positive".into(),
            ));
        }
        Ok(info)
    }

    /// Load tasks - supports both v2.0 (tasks.jsonl) and v3.0 (tasks.parquet)
    fn load_tasks(root: &Path) -> Result<Vec<TaskInfo>, DatasetError> {
        // Try v2.0 format first (tasks.jsonl)
        let jsonl_path = root.join("meta").join("tasks.jsonl");
        if jsonl_path.exists() {
            let file = File::open(&jsonl_path)?;
            let reader = BufReader::new(file);
            let mut tasks = Vec::new();

            for line in std::io::BufRead::lines(reader) {
                let line = line?;
                if !line.trim().is_empty() {
                    let task: TaskInfo = serde_json::from_str(&line)?;
                    tasks.push(task);
                }
            }
            return Ok(tasks);
        }

        // Try v3.0 format (tasks.parquet)
        let parquet_path = root.join("meta").join("tasks.parquet");
        if parquet_path.exists() {
            let file = File::open(&parquet_path)?;
            let reader = SerializedFileReader::new(file)?;
            let mut tasks = Vec::new();

            let mut iter = reader.get_row_iter(None)?;
            while let Some(record) = iter.next() {
                let record = record?;

                let task_index = row_u64(&record, "task_index").ok_or_else(|| {
                    DatasetError::InvalidFormat("task table has no task_index".into())
                })?;
                let task = row_string(&record, "task")
                    .or_else(|| row_string(&record, "__index_level_0__"))
                    .ok_or_else(|| {
                        DatasetError::InvalidFormat("task table has no description".into())
                    })?;

                tasks.push(TaskInfo { task_index, task });
            }
            return Ok(tasks);
        }

        // Tasks are optional
        Ok(vec![])
    }

    /// Read the actual episode index; lengths and task groups must never be inferred
    /// by dividing the total frame count (episodes rarely have equal lengths).
    fn load_episode_metadata(
        root: &Path,
        info: &DatasetInfo,
        tasks: &mut Vec<TaskInfo>,
    ) -> Result<Vec<EpisodeMetadata>, DatasetError> {
        let jsonl = root.join("meta/episodes.jsonl");
        let mut episodes = Vec::new();
        if jsonl.is_file() {
            for line in std::io::BufRead::lines(BufReader::new(File::open(jsonl)?)) {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(&line)?;
                let index = v["episode_index"]
                    .as_u64()
                    .ok_or_else(|| DatasetError::InvalidFormat("episode_index missing".into()))?;
                let length = v["length"]
                    .as_u64()
                    .ok_or_else(|| DatasetError::InvalidFormat("episode length missing".into()))?;
                let mut task_ids = Vec::new();
                if let Some(ts) = v["tasks"].as_array() {
                    for t in ts {
                        if let Some(id) = t.as_u64() {
                            task_ids.push(id);
                        } else if let Some(text) = t.as_str() {
                            task_ids.push(intern_task(tasks, text));
                        }
                    }
                }
                episodes.push(EpisodeMetadata {
                    episode_index: index,
                    tasks: task_ids,
                    length,
                    data_file: None,
                    video_files: HashMap::new(),
                });
            }
        } else {
            let legacy = root.join("meta/episodes.parquet");
            let files = if legacy.is_file() {
                vec![legacy]
            } else {
                parquet_files(&root.join("meta/episodes"))?
            };
            for path in files {
                let reader = SerializedFileReader::new(File::open(path)?)?;
                for row in reader.get_row_iter(None)? {
                    let row = row?;
                    let episode_index = row_u64(&row, "episode_index").ok_or_else(|| {
                        DatasetError::InvalidFormat("episode_index missing".into())
                    })?;
                    let length = row_u64(&row, "length").ok_or_else(|| {
                        DatasetError::InvalidFormat("episode length missing".into())
                    })?;
                    let data_file =
                        row_u64(&row, "data/chunk_index").zip(row_u64(&row, "data/file_index"));
                    let mut video_files = HashMap::new();
                    for (key, _) in &info.features {
                        let prefix = format!("videos/{key}");
                        if let (Some(c), Some(f), Some(t)) = (
                            row_u64(&row, &format!("{prefix}/chunk_index")),
                            row_u64(&row, &format!("{prefix}/file_index")),
                            row_f64(&row, &format!("{prefix}/from_timestamp")),
                        ) {
                            if !t.is_finite() || t < 0.0 {
                                return Err(DatasetError::InvalidFormat(
                                    "invalid video timestamp".into(),
                                ));
                            }
                            video_files.insert(key.clone(), (c, f, t));
                        }
                    }
                    let mut task_ids = Vec::new();
                    if let Some(parquet::record::Field::ListInternal(list)) =
                        row_field(&row, "tasks")
                    {
                        for t in list.elements() {
                            match t {
                                parquet::record::Field::Str(t) => {
                                    task_ids.push(intern_task(tasks, t))
                                }
                                parquet::record::Field::Long(i) if *i >= 0 => {
                                    task_ids.push(*i as u64)
                                }
                                parquet::record::Field::Int(i) if *i >= 0 => {
                                    task_ids.push(*i as u64)
                                }
                                _ => {}
                            }
                        }
                    }
                    episodes.push(EpisodeMetadata {
                        episode_index,
                        tasks: task_ids,
                        length,
                        data_file,
                        video_files,
                    });
                }
            }
        }
        episodes.sort_by_key(|e| e.episode_index);
        if episodes
            .windows(2)
            .any(|w| w[0].episode_index == w[1].episode_index)
        {
            return Err(DatasetError::InvalidFormat(
                "duplicate episode index".into(),
            ));
        }
        if episodes.len() as u64 != info.total_episodes {
            return Err(DatasetError::InvalidFormat(format!(
                "episode index has {} entries, info.json declares {}",
                episodes.len(),
                info.total_episodes
            )));
        }
        Ok(episodes)
    }

    /// Get task description by index
    pub fn get_task(&self, task_index: u64) -> Option<&str> {
        self.tasks
            .iter()
            .find(|t| t.task_index == task_index)
            .map(|t| t.task.as_str())
    }

    /// Calculate the cumulative frame offset for an episode in the video file.
    ///
    /// In LeRobot v3.0, all episodes are concatenated in a single MP4 file.
    /// This returns the starting frame index of the given episode.
    /// Check if dataset uses per-episode video files (v2.0) vs concatenated (v3.0)
    pub fn has_per_episode_videos(&self) -> bool {
        // v2.0 uses data_path pattern with episode_index
        // v3.0 uses file-XXX.mp4 pattern
        !self
            .info
            .codebase_version
            .trim_start_matches('v')
            .starts_with("3.")
    }

    pub fn calculate_video_frame_offset(&self, episode_index: u64) -> u64 {
        // For v2.0 datasets with per-episode video files, offset is always 0
        if self.has_per_episode_videos() {
            return 0;
        }
        self.get_episode_metadata(episode_index)
            .and_then(|ep| ep.video_files.values().next())
            .map(|(_, _, t)| (t * self.info.fps).round() as u64)
            .unwrap_or(0)
    }

    /// Load episode data from parquet files using Arrow batch reader for performance
    pub fn load_episode(&self, episode_index: u64) -> Result<EpisodeData, DatasetError> {
        use std::time::Instant;

        let t_start = Instant::now();

        // Calculate which chunk this episode is in
        let chunk_index = episode_index / self.info.chunks_size.max(1);
        let chunk_dir = format!("chunk-{:03}", chunk_index);

        // Find the data file
        let ep = self.get_episode_metadata(episode_index).ok_or_else(|| {
            DatasetError::InvalidFormat(format!("unknown episode {episode_index}"))
        })?;
        let data_path = self.find_episode_file(&chunk_dir, episode_index)?;
        let is_v3_format = data_path
            .file_name()
            .map(|n| n.to_string_lossy().starts_with("file-"))
            .unwrap_or(false);

        let t_find = t_start.elapsed();

        // Open file with Arrow reader (no projection for simplicity - columns are small)
        let file = File::open(&data_path)?;
        let reader = ParquetRecordBatchReaderBuilder::try_new(file)?
            .with_batch_size(ARROW_BATCH_SIZE)
            .build()?;

        let t_open = t_start.elapsed();

        // Get the schema from the reader
        let schema = reader.schema();

        // Find column indices by name in the batch schema
        let ep_col_idx = schema
            .fields()
            .iter()
            .position(|f| f.name() == "episode_index");
        let frame_col_idx = schema
            .fields()
            .iter()
            .position(|f| f.name() == "frame_index");
        let ts_col_idx = schema.fields().iter().position(|f| f.name() == "timestamp");
        let state_col_idx = schema
            .fields()
            .iter()
            .position(|f| f.name() == "observation.state");
        let action_col_idx = schema.fields().iter().position(|f| f.name() == "action");

        if is_v3_format && ep_col_idx.is_none() {
            return Err(DatasetError::InvalidFormat(
                "shared data file has no episode_index column".into(),
            ));
        }
        let t_schema = t_start.elapsed();

        let mut frames = Vec::new();
        let mut rows_processed = 0u64;

        for batch_result in reader {
            let batch = batch_result?;
            let num_rows = batch.num_rows();
            rows_processed += num_rows as u64;

            // Get episode column for filtering (v3.0 format)
            if is_v3_format
                && ep_col_idx.is_some_and(|idx| {
                    batch
                        .column(idx)
                        .as_any()
                        .downcast_ref::<Int64Array>()
                        .is_none()
                })
            {
                return Err(DatasetError::InvalidFormat(
                    "episode_index must be int64".into(),
                ));
            }
            let episode_filter: Option<Vec<bool>> = if is_v3_format {
                ep_col_idx.and_then(|idx| {
                    batch
                        .column(idx)
                        .as_any()
                        .downcast_ref::<Int64Array>()
                        .map(|arr| {
                            (0..num_rows)
                                .map(|i| arr.value(i) as u64 == episode_index)
                                .collect()
                        })
                })
            } else {
                None
            };

            // Extract columns
            let frame_arr = frame_col_idx
                .and_then(|idx| batch.column(idx).as_any().downcast_ref::<Int64Array>());

            // Keep the columns as `&dyn Array`: their concrete Arrow type
            // differs between v2.0 and v3.0 files.
            let ts_arr = ts_col_idx.map(|idx| batch.column(idx).as_ref());
            let state_arr = state_col_idx.map(|idx| batch.column(idx).as_ref());
            let action_arr = action_col_idx.map(|idx| batch.column(idx).as_ref());

            for row in 0..num_rows {
                // Filter by episode for v3.0 format
                if let Some(ref filter) = episode_filter {
                    if !filter[row] {
                        continue;
                    }
                }

                let frame_index = frame_arr
                    .map(|a| a.value(row) as u64)
                    .unwrap_or(frames.len() as u64);
                let timestamp = ts_arr
                    .and_then(|a| f64_at(a, row))
                    .unwrap_or(frame_index as f64 / self.info.fps);

                let state = state_arr.map(|a| float_vec_at(a, row)).unwrap_or_default();
                let action = action_arr.map(|a| float_vec_at(a, row)).unwrap_or_default();

                frames.push(EpisodeFrame {
                    frame_index,
                    timestamp,
                    state,
                    action,
                });
            }
        }

        let t_iterate = t_start.elapsed();

        // Sort frames by frame_index
        frames.sort_by_key(|f| f.frame_index);

        if frames.len() as u64 != ep.length {
            return Err(DatasetError::InvalidFormat(format!(
                "episode {episode_index}: read {} frames, expected {}",
                frames.len(),
                ep.length
            )));
        }
        // Calculate video frame offset
        let video_frame_offset = self.calculate_video_frame_offset(episode_index);

        // Find video paths
        let video_paths = self.find_video_paths(&chunk_dir, episode_index);

        let t_total = t_start.elapsed();

        // Timing log
        ::log::debug!("[Dataset] episode {} timing: find={:?}, open={:?}, schema={:?}, iterate={:?} (rows={}), total={:?}",
            episode_index, t_find, t_open, t_schema, t_iterate, rows_processed, t_total);

        Ok(EpisodeData {
            episode_index,
            frames,
            video_paths,
            video_frame_offset,
            video_frame_offsets: ep
                .video_files
                .iter()
                .map(|(k, (_, _, t))| (k.clone(), (t * self.info.fps).round() as u64))
                .collect(),
        })
    }

    /// Find episode data file with various naming conventions
    fn find_episode_file(
        &self,
        chunk_dir: &str,
        episode_index: u64,
    ) -> Result<PathBuf, DatasetError> {
        // v2.0 format: episode_XXX.parquet
        let v2_patterns = [
            format!("episode_{:06}.parquet", episode_index),
            format!("episode_{}.parquet", episode_index),
        ];

        for pattern in &v2_patterns {
            let path = self.root_path.join("data").join(chunk_dir).join(pattern);
            if path.exists() {
                return Ok(path);
            }
        }

        if let Some((chunk, file)) = self
            .get_episode_metadata(episode_index)
            .and_then(|e| e.data_file)
        {
            let path = self
                .root_path
                .join(format!("data/chunk-{chunk:03}/file-{file:03}.parquet"));
            if path.is_file() {
                return Ok(path);
            }
            return Err(DatasetError::MissingFile(path.display().to_string()));
        }
        // Older v3 exports may lack file indexes. Search the episode column,
        // never assume the first file contains the requested episode.
        for path in parquet_files(&self.root_path.join("data"))? {
            let reader = SerializedFileReader::new(File::open(&path)?)?;
            for row in reader.get_row_iter(None)? {
                if row_u64(&row?, "episode_index") == Some(episode_index) {
                    return Ok(path);
                }
            }
        }

        Err(DatasetError::MissingFile(format!(
            "data/{}/episode or file parquet",
            chunk_dir
        )))
    }

    /// Find column index by name in schema
    fn find_column_index(schema: &parquet::schema::types::Type, name: &str) -> Option<usize> {
        if let parquet::schema::types::Type::GroupType { fields, .. } = schema {
            for (i, field) in fields.iter().enumerate() {
                if field.name() == name {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Extract integer value from a parquet row
    fn extract_int(record: &parquet::record::Row, col_idx: usize) -> Option<i64> {
        use parquet::record::Field;
        let fields: Vec<_> = record.get_column_iter().collect();
        if col_idx >= fields.len() {
            return None;
        }
        let (_, field) = &fields[col_idx];
        match field {
            Field::Long(v) => Some(*v),
            Field::Int(v) => Some(*v as i64),
            _ => None,
        }
    }

    /// Find video paths for an episode
    ///
    /// Supports multiple LeRobot formats:
    /// - v3.0: videos/{camera_name}/chunk-{chunk_index}/file-{file_index}.mp4
    /// - v2.1: videos/chunk-{chunk_index}/{video_key}/episode_{episode_index}.mp4
    /// - v2.0: videos/chunk-{chunk_index}/{camera_name}_{episode_index}.mp4
    fn find_video_paths(&self, chunk_dir: &str, episode_index: u64) -> HashMap<String, PathBuf> {
        let mut video_paths = HashMap::new();

        // Get video feature names from dataset info
        let video_features: Vec<&String> = self
            .info
            .features
            .iter()
            .filter(|(_, feature)| feature.dtype == "video" || feature.dtype.contains("video"))
            .map(|(name, _)| name)
            .collect();

        if let Some(ep) = self.get_episode_metadata(episode_index) {
            for (key, (chunk, file, _)) in &ep.video_files {
                let path = self
                    .root_path
                    .join(format!("videos/{key}/chunk-{chunk:03}/file-{file:03}.mp4"));
                if path.is_file() {
                    video_paths.insert(key.clone(), path);
                }
            }
        }

        // Try v2.1 format: videos/chunk-XXX/{video_key}/episode_{episode_index}.mp4
        if video_paths.is_empty() {
            for cam_name in &video_features {
                let cam_dir = self.root_path.join("videos").join(chunk_dir).join(cam_name);
                if cam_dir.exists() {
                    for pattern in [
                        format!("episode_{:06}.mp4", episode_index),
                        format!("episode_{}.mp4", episode_index),
                    ] {
                        let video_path = cam_dir.join(&pattern);
                        if video_path.exists() {
                            video_paths.insert(cam_name.to_string(), video_path);
                            break;
                        }
                    }
                }
            }
        }

        // Try v2.0 format: videos/chunk-XXX/{camera_name}_{episode_index}.mp4
        if video_paths.is_empty() {
            let videos_dir = self.root_path.join("videos").join(chunk_dir);
            if videos_dir.exists() {
                for cam_name in &video_features {
                    for pattern in [
                        format!("{}_{:06}.mp4", cam_name, episode_index),
                        format!("{}_{}.mp4", cam_name, episode_index),
                    ] {
                        let video_path = videos_dir.join(&pattern);
                        if video_path.exists() {
                            video_paths.insert(cam_name.to_string(), video_path);
                            break;
                        }
                    }
                }
            }
        }

        video_paths
    }

    /// Get the number of episodes
    pub fn num_episodes(&self) -> usize {
        self.episodes.len()
    }

    /// Get episode by index
    pub fn get_episode_metadata(&self, index: u64) -> Option<&EpisodeMetadata> {
        self.episodes.iter().find(|e| e.episode_index == index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dataset_info_default() {
        let info = DatasetInfo::default();
        assert_eq!(info.fps, 30.0);
        assert_eq!(info.codebase_version, "2.0");
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use crate::data::lerobot_writer::{DatasetSpec, DatasetWriter, FrameRecord};
    use arrow_array::{Float64Array, Int64Array, RecordBatch};
    use arrow_schema::{DataType, Field, Schema};
    use parquet::arrow::ArrowWriter;
    use std::{fs, sync::Arc};
    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dorobot-reader-{name}-{}", std::process::id()))
    }
    fn make(root: &Path) {
        let _ = fs::remove_dir_all(root);
        let mut w = DatasetWriter::create(
            root,
            DatasetSpec {
                robot_type: "so100".into(),
                fps: 30.0,
                camera_keys: vec![],
                joint_names: vec!["joint".into()],
            },
        )
        .unwrap();
        for (task, n) in [("pick", 3), ("place", 7)] {
            let mut ep = w.begin_episode(task).unwrap();
            for i in 0..n {
                ep.push(FrameRecord {
                    state: vec![i as f32],
                    action: vec![0.0],
                })
                .unwrap();
            }
            w.commit(ep).unwrap();
        }
    }
    #[test]
    fn jsonl_preserves_unequal_episode_lengths_and_string_tasks() {
        let root = root("v2");
        make(&root);
        let ds = LeRobotDataset::open(&root).unwrap();
        assert_eq!(
            ds.episodes.iter().map(|e| e.length).collect::<Vec<_>>(),
            [3, 7]
        );
        assert_eq!(ds.get_task(ds.episodes[1].tasks[0]), Some("place"));
        assert_eq!(ds.load_episode(1).unwrap().frames.len(), 7);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn v3_uses_indexed_data_files_and_per_camera_offsets() {
        let root = root("v3");
        make(&root);
        fs::create_dir_all(root.join("data/chunk-004")).unwrap();
        for i in 0..2 {
            fs::rename(
                root.join(format!("data/chunk-000/episode_{i:06}.parquet")),
                root.join(format!("data/chunk-004/file-{:03}.parquet", i + 12)),
            )
            .unwrap();
        }
        let mut info: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("meta/info.json")).unwrap()).unwrap();
        info["codebase_version"] = "v3.0".into();
        for camera in ["front", "wrist"] {
            info["features"][camera] = serde_json::json!({"dtype":"video", "shape":[480,640,3]});
        }
        fs::write(root.join("meta/info.json"), info.to_string()).unwrap();
        fs::remove_file(root.join("meta/episodes.jsonl")).unwrap();
        let fields = [
            "episode_index",
            "length",
            "data/chunk_index",
            "data/file_index",
            "videos/front/chunk_index",
            "videos/front/file_index",
            "videos/wrist/chunk_index",
            "videos/wrist/file_index",
        ];
        let mut schema: Vec<_> = fields
            .iter()
            .map(|n| Field::new(*n, DataType::Int64, false))
            .collect();
        schema.extend([
            Field::new("videos/front/from_timestamp", DataType::Float64, false),
            Field::new("videos/wrist/from_timestamp", DataType::Float64, false),
        ]);
        let schema = Arc::new(Schema::new(schema));
        let mut columns: Vec<Arc<dyn Array>> = [
            vec![0, 1],
            vec![3, 7],
            vec![4, 4],
            vec![12, 13],
            vec![2, 2],
            vec![8, 9],
            vec![6, 6],
            vec![3, 4],
        ]
        .into_iter()
        .map(|v| Arc::new(Int64Array::from(v)) as Arc<dyn Array>)
        .collect();
        columns.extend([
            Arc::new(Float64Array::from(vec![0.0, 2.5])) as Arc<dyn Array>,
            Arc::new(Float64Array::from(vec![0.0, 1.25])),
        ]);
        fs::create_dir_all(root.join("meta/episodes/chunk-002")).unwrap();
        let mut writer = ArrowWriter::try_new(
            File::create(root.join("meta/episodes/chunk-002/file-017.parquet")).unwrap(),
            schema.clone(),
            None,
        )
        .unwrap();
        writer
            .write(&RecordBatch::try_new(schema, columns).unwrap())
            .unwrap();
        writer.close().unwrap();
        for path in [
            "videos/front/chunk-002/file-009.mp4",
            "videos/wrist/chunk-006/file-004.mp4",
        ] {
            let p = root.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "video fixture").unwrap();
        }
        let ds = LeRobotDataset::open(&root).unwrap();
        let ep = ds.load_episode(1).unwrap();
        assert_eq!(ep.frames.len(), 7);
        assert_eq!(ep.video_frame_offsets["front"], 75);
        assert_eq!(ep.video_frame_offsets["wrist"], 38);
        assert!(ep.video_paths["front"].ends_with("file-009.mp4"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn missing_chunk_size_uses_standard_default() {
        let info: DatasetInfo = serde_json::from_str(r#"{"fps":30}"#).unwrap();
        assert_eq!(info.chunks_size, 1000);
    }
    #[test]
    fn invalid_fps_and_missing_index_are_rejected() {
        let root = root("invalid");
        make(&root);
        fs::remove_file(root.join("meta/episodes.jsonl")).unwrap();
        assert!(LeRobotDataset::open(&root).is_err());
        fs::write(root.join("meta/info.json"), r#"{"fps":0}"#).unwrap();
        assert!(LeRobotDataset::open(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
