//! Read a recorded experiment without changing simulation artifacts.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub fn read(p: &Path) -> Result<Value> {
    Ok(serde_json::from_reader(BufReader::new(File::open(p)?))?)
}
pub fn files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut all = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    all.sort();
    Ok(all)
}

#[derive(Clone)]
pub struct Entry {
    pub batch: PathBuf,
    pub batch_name: String,
    pub scene_name: String,
    pub instruction: String,
    pub row: Value,
    pub notes: Value,
}
impl Entry {
    pub fn id(&self) -> &str {
        self.row["run_id"].as_str().unwrap_or("")
    }
    pub fn trial(&self) -> PathBuf {
        self.batch.join("trials").join(self.id())
    }
    pub fn revisions(&self) -> Result<Vec<PathBuf>> {
        Ok(files(&self.batch.join("annotations").join(self.id()))?
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect())
    }
    pub fn matches(&self, query: &str) -> bool {
        let haystack = format!(
            "{} {} {} {} {} {}",
            self.batch_name,
            self.scene_name,
            self.instruction,
            self.id(),
            self.row,
            self.notes
        )
        .to_lowercase();
        query
            .split_whitespace()
            .all(|term| haystack.contains(&term.to_lowercase()))
    }
    pub fn label(&self) -> String {
        format!(
            "{}  ·  {}  ·  {}",
            self.notes["name"].as_str().unwrap_or(&self.batch_name),
            self.row["status"].as_str().unwrap_or("unknown"),
            self.id().get(..8).unwrap_or("")
        )
    }
    pub fn variants(&self) -> Result<Vec<Value>> {
        let mut list = files(&self.batch.join("variants").join(self.id()))?
            .into_iter()
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .map(|p| read(&p))
            .collect::<Result<Vec<_>>>()?;
        list.sort_by(|a, b| a["created"].as_str().cmp(&b["created"].as_str()));
        Ok(list)
    }
}
pub fn catalog(root: &Path) -> Result<Vec<Entry>> {
    let mut entries = vec![];
    for batch in files(root)? {
        if !batch.join("batch.json").is_file()
            || batch
                .file_name()
                .is_some_and(|s| s.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        let job = read(&batch.join("batch.json"))?;
        let task = read(&batch.join("controller/bound-task.json"))?;
        let name = job["configuration"]["name"]
            .as_str()
            .unwrap_or("Experiment");
        for row in job["repetitions"]
            .as_array()
            .context("Missing trial index")?
        {
            let mut e = Entry {
                batch: batch.clone(),
                batch_name: name.into(),
                scene_name: task["scene"]["name"].as_str().unwrap_or("").into(),
                instruction: task["instruction"].as_str().unwrap_or("").into(),
                row: row.clone(),
                notes: json!({"name":format!("{} · {}",name,row["repetition"]),"notes":"","tags":[],"fields":{},"revision":0}),
            };
            if let Some(p) = e.revisions()?.last() {
                e.notes = read(p)?;
            }
            entries.push(e);
        }
    }
    entries.sort_by(|a, b| {
        b.row["created"]
            .as_str()
            .cmp(&a.row["created"].as_str())
            .then(a.id().cmp(b.id()))
    });
    Ok(entries)
}

pub struct Replay {
    dir: PathBuf,
    pub steps: Vec<u64>,
    pub camera_frames: usize,
    pub tick_ns: u64,
    pub interval_ms: u64,
    pub phases: Vec<Value>,
    pub events: Vec<Value>,
    pub cameras: Vec<String>,
    trace: Option<BufReader<File>>,
    offsets: Vec<u64>,
}
impl Replay {
    pub fn open(entry: &Entry) -> Result<Self> {
        let dir = entry.trial();
        let g1 = entry.row["robot"] == "G1";
        let tick_ns = if g1 { 5_000_000 } else { 10_000_000 };
        let record = dir.join("recording.json");
        let rec = if record.is_file() {
            read(&record)?
        } else {
            Value::Null
        };
        let mut steps = rec["samples"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|v| v["step"].as_u64().context("Missing sample time"))
            .collect::<Result<Vec<_>>>()?;
        let cameras = rec["cameras"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|s| {
                if g1 {
                    *s == "operator"
                } else {
                    ["head", "left_wrist", "right_wrist"].contains(s)
                }
            })
            .map(String::from)
            .collect();
        let phases = if dir.join("phases.json").is_file() {
            read(&dir.join("phases.json"))?
                .as_array()
                .cloned()
                .unwrap_or_default()
        } else if g1 && dir.join("result.json").is_file() {
            let mut marks = vec![json!({"name":"standing reach","tick":1})];
            let contacts = dir.join("impact-contacts.json");
            if contacts.is_file() {
                if let Some(t) = read(&contacts)?.get(0).and_then(|r| r["step"].as_u64()) {
                    marks.push(json!({"name":"block contact","tick":t}));
                }
            }
            if let Some(t) =
                read(&dir.join("result.json"))?["disturbance"]["recovered_step"].as_u64()
            {
                marks.push(json!({"name":"stable recovery","tick":t}));
            }
            marks
        } else {
            vec![]
        };
        let history = dir.join("agent-history.jsonl");
        let mut events = vec![];
        if history.is_file() {
            for line in BufReader::new(File::open(history)?).lines() {
                let line = line?;
                events.push(serde_json::from_str(&line)?);
            }
        }
        let mut trace = None;
        let mut offsets = vec![];
        let trace_path = dir.join("robot-trace.jsonl");
        if trace_path.is_file() {
            let mut file = BufReader::new(File::open(trace_path)?);
            let mut pos = 0;
            let mut line = String::new();
            loop {
                line.clear();
                let n = file.read_line(&mut line)?;
                if n == 0 {
                    break;
                }
                offsets.push(pos);
                pos += n as u64;
            }
            trace = Some(file);
        }
        let camera_frames = steps.len();
        if steps.is_empty() && !offsets.is_empty() {
            // Recorded traces use a 100 Hz clock. Show five points per second in the player.
            steps = (1..=offsets.len() as u64)
                .step_by(if g1 { 40 } else { 20 })
                .collect();
            if steps.last() != Some(&(offsets.len() as u64)) {
                steps.push(offsets.len() as u64);
            }
        }
        Ok(Self {
            dir,
            camera_frames,
            tick_ns,
            interval_ms: rec["fps"]
                .as_u64()
                .filter(|n| *n > 0)
                .map_or(200, |fps| (1000 / fps).max(1)),
            steps,
            cameras,
            phases,
            events,
            trace,
            offsets,
        })
    }
    pub fn index_at(&self, tick: u64) -> usize {
        self.steps
            .partition_point(|v| *v < tick)
            .min(self.steps.len().saturating_sub(1))
    }
    pub fn rgb(&self, index: usize, camera: &str) -> Result<image::RgbImage> {
        anyhow::ensure!(
            self.cameras.iter().any(|s| s == camera) && index < self.steps.len(),
            "No recorded RGB at this position"
        );
        let root = self.dir.join("camera");
        let root = if camera == "operator" {
            root
        } else {
            root.join(camera)
        };
        Ok(image::open(root.join(format!("{index:04}.png")))?.into_rgb8())
    }
    pub fn trace_at(&mut self, tick: u64) -> Result<Option<Value>> {
        let Some(file) = &mut self.trace else {
            return Ok(None);
        };
        let Some(offset) = tick
            .checked_sub(1)
            .and_then(|i| self.offsets.get(i as usize))
        else {
            return Ok(None);
        };
        file.seek(SeekFrom::Start(*offset))?;
        let mut line = String::new();
        file.read_line(&mut line)?;
        let point: Value = serde_json::from_str(&line)?;
        anyhow::ensure!(
            point["physics_step"].as_u64() == Some(tick),
            "Trace time differs from camera time"
        );
        Ok(Some(point))
    }
}
