//! One local Nexus job shared by setup, collection, replay and policy panels.
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NexusKind {
    Capture,
    Train,
    Rollout,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct NexusSettings {
    pub python: String,
    pub leader_port: String,
    pub calibration_dir: String,
    pub dataset: String,
    pub checkpoint: String,
    pub output_root: String,
    pub train_steps: String,
    pub rollout_steps: String,
}

impl Default for NexusSettings {
    fn default() -> Self {
        let env = |key| std::env::var(key).unwrap_or_default();
        Self {
            python: std::env::var("DOROBOT_LEROBOT_PYTHON").unwrap_or_else(|_| "python3".into()),
            leader_port: env("DOROBOT_SO101_LEADER"),
            calibration_dir: env("DOROBOT_SO101_CALIBRATION"),
            dataset: String::new(),
            checkpoint: String::new(),
            output_root: crate::runtime::user_data()
                .join("nexus")
                .display()
                .to_string(),
            train_steps: "1000".into(),
            rollout_steps: "200".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum NexusIntent {
    Configure(NexusSettings),
    StartCapture,
    Reset,
    Pause,
    Resume,
    Step,
    Record,
    Finish,
    Save,
    Discard,
    Stop,
    Train,
    Run,
    UseOpenDataset,
    OpenSavedDataset,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NexusSnapshot {
    pub version: u32,
    pub phase: String,
    pub message: String,
    #[serde(default)]
    pub ack: u64,
    #[serde(default)]
    pub frames: u64,
    #[serde(default)]
    pub steps: u64,
    #[serde(default)]
    pub total_steps: u64,
    pub loss: Option<f64>,
    pub dataset: Option<PathBuf>,
    pub checkpoint: Option<PathBuf>,
    pub video: Option<PathBuf>,
    pub task_success: Option<bool>,
    pub source_mode: Option<String>,
    pub positions: Option<[f32; 6]>,
    pub targets: Option<[f32; 6]>,
}

#[derive(Debug)]
pub struct NexusPreview {
    pub sequence: u64,
    pub width: u16,
    pub height: u16,
    pub rgb: Vec<u8>,
}

impl NexusPreview {
    const SIZE: usize = 12 + 640 * 480 * 3;
    fn header(bytes: &[u8], previous: Option<u64>) -> Option<(u64, u16, u16)> {
        let sequence = u64::from_be_bytes(bytes.get(..8)?.try_into().ok()?);
        let width = u16::from_be_bytes(bytes.get(8..10)?.try_into().ok()?);
        let height = u16::from_be_bytes(bytes.get(10..12)?.try_into().ok()?);
        if previous.is_some_and(|seq| sequence <= seq) || (width, height) != (640, 480) {
            return None;
        }
        Some((sequence, width, height))
    }

    pub fn decode(bytes: &[u8], previous: Option<u64>) -> Option<Self> {
        let (sequence, width, height) = Self::header(bytes, previous)?;
        if bytes.len() != Self::SIZE {
            return None;
        }
        Some(Self {
            sequence,
            width,
            height,
            rgb: bytes[12..].to_vec(),
        })
    }

    fn read(path: &Path, previous: Option<u64>) -> Option<Self> {
        // Open once so a concurrent rename cannot mix header and pixels.
        let mut file = File::open(path).ok()?;
        let mut header = [0; 12];
        file.read_exact(&mut header).ok()?;
        let (sequence, width, height) = Self::header(&header, previous)?;
        if file.metadata().ok()?.len() != Self::SIZE as u64 {
            return None;
        }
        let mut rgb = vec![0; Self::SIZE - 12];
        file.read_exact(&mut rgb).ok()?;
        Some(Self {
            sequence,
            width,
            height,
            rgb,
        })
    }
}

#[derive(Debug)]
pub struct NexusState {
    pub settings: NexusSettings,
    pub snapshot: NexusSnapshot,
    pub preview: Option<Arc<NexusPreview>>,
    pub kind: Option<NexusKind>,
    pub job_dir: Option<PathBuf>,
    pub busy: bool,
    pub command_pending: bool,
    pub recent_log: Vec<String>,
    pub loss_history: Vec<(u64, f64)>,
}
impl Default for NexusState {
    fn default() -> Self {
        Self {
            settings: NexusSettings::default(),
            snapshot: NexusSnapshot {
                version: 1,
                phase: "stopped".into(),
                ..Default::default()
            },
            preview: None,
            kind: None,
            job_dir: None,
            busy: false,
            command_pending: false,
            recent_log: Vec::new(),
            loss_history: Vec::new(),
        }
    }
}
impl NexusState {
    pub fn allows(&self, intent: &NexusIntent) -> bool {
        use NexusIntent::*;
        let phase = self.snapshot.phase.as_str();
        match intent {
            Configure(_) | StartCapture | Train | Run | UseOpenDataset => !self.busy,
            OpenSavedDataset => self.snapshot.dataset.is_some(),
            Stop => self.busy && phase != "stopping",
            _ if !self.busy || self.command_pending || self.kind != Some(NexusKind::Capture) => {
                false
            }
            Reset => matches!(phase, "reset_required" | "ready" | "paused" | "saved"),
            Record | Pause => phase == "ready",
            Step | Resume => phase == "paused",
            Finish => phase == "recording",
            Save | Discard => phase == "review",
        }
    }
}

#[derive(Default)]
struct Updates {
    snapshot: Option<NexusSnapshot>,
    preview: Option<Arc<NexusPreview>>,
    log: Option<Vec<String>>,
    loss: Vec<(u64, f64)>,
    done: bool,
}
struct Job {
    requests: mpsc::Sender<(u64, &'static str)>,
    thread: JoinHandle<()>,
    updates: Arc<Mutex<Updates>>,
}
#[derive(Default)]
pub(super) struct NexusRuntime {
    pub state: NexusState,
    job: Option<Job>,
    next_id: u64,
    pending: Option<u64>,
}

impl NexusRuntime {
    pub fn dispatch(&mut self, intent: NexusIntent) {
        use NexusIntent::*;
        if !self.state.allows(&intent) {
            return;
        }
        match intent {
            Configure(settings) => self.state.settings = settings,
            StartCapture => self.start(NexusKind::Capture),
            Train => self.start(NexusKind::Train),
            Run => self.start(NexusKind::Rollout),
            UseOpenDataset | OpenSavedDataset => {} // The FileBackend owns the library.
            other => {
                let action = match other {
                    Reset => "reset",
                    Pause => "pause",
                    Resume => "resume",
                    Step => "step",
                    Record => "record",
                    Finish => "finish",
                    Save => "save",
                    Discard => "discard",
                    Stop => "stop",
                    _ => unreachable!(),
                };
                self.next_id += 1;
                if let Some(job) = &self.job {
                    if job.requests.send((self.next_id, action)).is_ok() {
                        self.pending = Some(self.next_id);
                        self.state.command_pending = true;
                        if action == "stop" {
                            self.state.snapshot.phase = "stopping".into();
                        }
                    }
                }
            }
        }
    }

    fn start(&mut self, kind: NexusKind) {
        let settings = self.state.settings.clone();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let folder = PathBuf::from(&settings.output_root)
            .join(format!("job-{}-{stamp}", std::process::id()));
        let (sender, receiver) = mpsc::channel();
        let updates = Arc::new(Mutex::new(Updates::default()));
        let shared = updates.clone();
        self.state.snapshot = NexusSnapshot {
            version: 1,
            phase: "starting".into(),
            message: "Starting local worker".into(),
            dataset: self.state.snapshot.dataset.take(),
            checkpoint: self.state.snapshot.checkpoint.take(),
            ..Default::default()
        };
        self.state.kind = Some(kind);
        self.state.job_dir = Some(folder.clone());
        self.state.preview = None;
        self.state.busy = true;
        self.state.command_pending = false;
        self.state.recent_log.clear();
        if kind == NexusKind::Train {
            self.state.loss_history.clear();
        }
        self.next_id = 0;
        self.pending = None;
        let spawn = thread::Builder::new()
            .name("nexus-supervisor".into())
            .spawn(move || {
                let result = launch(&settings, kind, &folder)
                    .and_then(|child| supervise(child, &folder, receiver, &shared));
                let mut updates = shared.lock().unwrap();
                if let Err(error) = result {
                    let snapshot = updates.snapshot.get_or_insert_with(Default::default);
                    snapshot.version = 1;
                    snapshot.phase = "failed".into();
                    snapshot.message = error.to_string();
                    snapshot.task_success = None;
                }
                updates.done = true;
            });
        match spawn {
            Ok(thread) => {
                self.job = Some(Job {
                    requests: sender,
                    thread,
                    updates,
                })
            }
            Err(e) => {
                self.state.busy = false;
                self.state.snapshot.phase = "failed".into();
                self.state.snapshot.message = e.to_string();
            }
        }
    }

    pub fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        let Ok(mut lock) = job.updates.try_lock() else {
            return;
        };
        let updates = std::mem::take(&mut *lock);
        drop(lock);
        if let Some(mut snapshot) = updates.snapshot {
            if let Some(path) = &snapshot.dataset {
                if self.state.snapshot.dataset.as_ref() != Some(path) {
                    self.state.settings.dataset = path.display().to_string();
                }
            } else {
                snapshot.dataset.clone_from(&self.state.snapshot.dataset);
            }
            if let Some(path) = &snapshot.checkpoint {
                if self.state.snapshot.checkpoint.as_ref() != Some(path) {
                    self.state.settings.checkpoint = path.display().to_string();
                }
            } else {
                snapshot
                    .checkpoint
                    .clone_from(&self.state.snapshot.checkpoint);
            }
            if self.pending.is_some_and(|id| snapshot.ack >= id) {
                self.pending = None;
                self.state.command_pending = false;
            }
            if self.state.snapshot.phase == "stopping"
                && !updates.done
                && !matches!(snapshot.phase.as_str(), "completed" | "stopped" | "failed")
            {
                snapshot.phase = "stopping".into();
            }
            self.state.snapshot = snapshot;
        }
        if let Some(preview) = updates.preview {
            self.state.preview = Some(preview);
        }
        if let Some(log) = updates.log {
            self.state.recent_log = log;
        }
        self.state.loss_history.extend(updates.loss);
        let excess = self.state.loss_history.len().saturating_sub(200);
        self.state.loss_history.drain(..excess);
        if updates.done {
            self.state.busy = false;
            self.state.command_pending = false;
            self.pending = None;
            let job = self.job.take().unwrap();
            let _ = job.thread.join();
        }
    }
}
impl Drop for NexusRuntime {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            let _ = job.requests.send((self.next_id + 1, "stop"));
            drop(job.requests);
            let _ = job.thread.join();
        }
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn launch(settings: &NexusSettings, kind: NexusKind, folder: &Path) -> io::Result<Child> {
    let root = crate::runtime::root();
    let worker = root.join("tools/nexus_workflow/worker.py");
    let model = root.join("data/so101/so101_new_calib.urdf");
    let scene = root.join("data/so101/cube-to-plate.json");
    let runtime = std::env::var_os("DOROBOT_SO101_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            crate::runtime::resources()
                .map(|p| p.join("runtime/bin/dorobot-so101-session"))
                .unwrap_or_else(|| root.join("runtime/target/release/dorobot-so101-session"))
        });
    let synthetic = std::env::var("DOROBOT_NEXUS_SYNTHETIC").as_deref() == Ok("1");
    let (name, steps) = match kind {
        NexusKind::Capture => ("capture", 1),
        NexusKind::Train => ("train", parse_steps(&settings.train_steps)?),
        NexusKind::Rollout => ("rollout", parse_steps(&settings.rollout_steps)?),
    };
    if !folder.is_absolute() {
        return Err(invalid("Choose an absolute output directory."));
    }
    if !worker.is_file() {
        return Err(invalid(format!(
            "Missing LeRobot worker at {}",
            worker.display()
        )));
    }
    if kind != NexusKind::Train {
        for (label, path) in [
            ("SO-101 runtime", &runtime),
            ("SO-101 model", &model),
            ("Cube-to-plate scene", &scene),
        ] {
            if !path.is_file() || !path.is_absolute() {
                return Err(invalid(format!("Missing {label} at {}", path.display())));
            }
        }
    }
    if kind == NexusKind::Capture && !synthetic {
        if !Path::new(&settings.leader_port).is_absolute()
            || !Path::new(&settings.leader_port).exists()
        {
            return Err(invalid("Choose the connected SO-101 leader serial device."));
        }
        let calibration = Path::new(&settings.calibration_dir);
        let calibration_file =
            calibration.extension().is_some_and(|ext| ext == "json") && calibration.is_file();
        if !calibration.is_absolute() || !(calibration_file || calibration.is_dir()) {
            return Err(invalid(
                "Choose the saved SO-101 leader calibration JSON or its folder.",
            ));
        }
    }
    if kind == NexusKind::Train && !absolute_dir(&settings.dataset) {
        return Err(invalid(
            "Select an existing local LeRobot v3 dataset or collection directory.",
        ));
    }
    if kind == NexusKind::Rollout && !absolute_dir(&settings.checkpoint) {
        return Err(invalid(
            "Select an existing local ACT checkpoint directory.",
        ));
    }
    fs::create_dir_all(
        folder
            .parent()
            .ok_or_else(|| invalid("Invalid output directory"))?,
    )?;
    fs::create_dir(folder)?;
    let config = folder.join("config.json");
    fs::write(
        &config,
        serde_json::to_vec_pretty(&json!({
            "version":1,"kind":name,"runtime":runtime,"model":model,"scene":scene,
            "leader_port":settings.leader_port,"calibration_dir":settings.calibration_dir,"steps":steps,
            "dataset":settings.dataset,"checkpoint":settings.checkpoint,"output_root":folder,"synthetic":synthetic,
        }))?,
    )?;
    let log = File::create(folder.join("worker.log"))?;
    let mut cmd = Command::new(&settings.python);
    cmd.arg("-u")
        .arg(worker)
        .arg(config)
        .env("DOROBOT_NEXUS_STUDIO_PID", std::process::id().to_string())
        .current_dir(folder)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    crate::runtime::configure(&mut cmd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().map_err(|e| {
        invalid(format!(
            "Could not start LeRobot Python {}: {e}",
            settings.python
        ))
    })
}
fn absolute_dir(path: &str) -> bool {
    Path::new(path).is_absolute() && Path::new(path).is_dir()
}
fn parse_steps(value: &str) -> io::Result<u64> {
    value
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| invalid("Steps must be a positive integer."))
}

/// Keep all descendants in the worker's process group, including on errors
/// reading the control channel or if Python exits before its child.
struct WorkerGroup(Child);
impl Drop for WorkerGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            extern "C" {
                fn kill(pid: i32, signal: i32) -> i32;
            }
            kill(-(self.0.id() as i32), 9);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn finish_status(snapshot: &mut NexusSnapshot, exit: ExitStatus, cancelled: bool) {
    if snapshot.phase == "failed" || (snapshot.phase == "completed" && exit.success()) {
        return;
    }
    if cancelled {
        snapshot.phase = "stopped".into();
        snapshot.message = "Local job stopped.".into();
        snapshot.task_success = None;
    } else if !exit.success() || !matches!(snapshot.phase.as_str(), "completed" | "stopped") {
        snapshot.phase = "failed".into();
        snapshot.message =
            format!("Worker exited ({exit}) before a valid completion. See worker.log.");
        snapshot.task_success = None;
    }
}

fn log_tail(path: &Path) -> Option<Vec<String>> {
    let mut f = File::open(path).ok()?;
    let start = f.metadata().ok()?.len().saturating_sub(8192);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    f.take(8192).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<_> = text
        .lines()
        .rev()
        .take(15)
        .map(|s| s.chars().take(512).collect())
        .collect();
    lines.reverse();
    Some(lines)
}

fn read_status(path: &Path) -> Option<NexusSnapshot> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > 64 * 1024 {
        return None;
    }
    let mut s: NexusSnapshot = serde_json::from_reader(file.take(64 * 1024)).ok()?;
    if s.version != 1
        || !matches!(
            s.phase.as_str(),
            "starting"
                | "reset_required"
                | "resetting"
                | "ready"
                | "paused"
                | "recording"
                | "review"
                | "saving"
                | "saved"
                | "training"
                | "running"
                | "stopping"
                | "stopped"
                | "completed"
                | "failed"
        )
    {
        return None;
    }
    s.dataset = s
        .dataset
        .filter(|p| p.is_absolute() && p.join("meta/info.json").is_file());
    s.checkpoint = s.checkpoint.filter(|p| {
        p.is_absolute() && p.join("config.json").is_file() && p.join("model.safetensors").is_file()
    });
    s.video = s.video.filter(|p| p.is_absolute() && p.is_file());
    Some(s)
}

fn supervise(
    child: Child,
    folder: &Path,
    commands: mpsc::Receiver<(u64, &'static str)>,
    shared: &Mutex<Updates>,
) -> io::Result<()> {
    let mut group = WorkerGroup(child);
    let mut status = NexusSnapshot {
        version: 1,
        phase: "starting".into(),
        message: "Starting local worker".into(),
        ..Default::default()
    };
    let status_path = folder.join("status.json");
    let preview_path = folder.join("preview.rgb");
    let mut sequence = None;
    let mut stop = None;
    let mut previous_loss = None;
    let mut last_log = None;
    loop {
        for (id, action) in commands.try_iter() {
            let tmp = folder.join("command.tmp");
            fs::write(
                &tmp,
                serde_json::to_vec(&json!({"version":1,"id":id,"action":action}))?,
            )?;
            fs::rename(tmp, folder.join("command.json"))?;
            if action == "stop" && stop.is_none() {
                stop = Some(Instant::now());
            }
        }
        let exit = group.0.try_wait()?;
        if let Some(mut next) = read_status(&status_path) {
            if next.dataset.is_none() {
                next.dataset = status.dataset.take();
            }
            // A checkpoint is selectable only after the trainer validates it
            // and the worker process exits successfully.
            if next.phase != "completed" || !exit.is_some_and(|code| code.success()) {
                next.checkpoint = None;
            }
            if next.checkpoint.is_none() {
                next.checkpoint = status.checkpoint.take();
            }
            status = next;
        }
        let expired = stop.is_some_and(|t| t.elapsed() >= Duration::from_secs(5));
        if let Some(exit) = exit {
            finish_status(&mut status, exit, stop.is_some());
        } else if expired {
            status.phase = "stopped".into();
            status.message = "Stopped the unresponsive worker and its subprocesses.".into();
            status.task_success = None;
        }
        let preview = NexusPreview::read(&preview_path, sequence).map(Arc::new);
        if let Some(p) = &preview {
            sequence = Some(p.sequence);
        }
        let log = if exit.is_some()
            || last_log.is_none_or(|t: Instant| t.elapsed() >= Duration::from_millis(400))
        {
            last_log = Some(Instant::now());
            let mut lines = log_tail(&folder.join("worker.log")).unwrap_or_default();
            if let Some(training) = log_tail(&folder.join("training.log")) {
                lines.extend(training);
                lines.drain(..lines.len().saturating_sub(15));
            }
            Some(lines)
        } else {
            None
        };
        let mut update = shared.lock().unwrap();
        update.snapshot = Some(status.clone());
        if let Some(preview) = preview {
            update.preview = Some(preview);
        }
        if let Some(log) = log {
            update.log = Some(log);
        }
        if let Some(loss) = status.loss {
            if previous_loss != Some((status.steps, loss)) {
                previous_loss = Some((status.steps, loss));
                if update.loss.len() == 200 {
                    update.loss.remove(0);
                }
                update.loss.push((status.steps, loss));
            }
        }
        drop(update);
        if exit.is_some() || expired {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(35));
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    #[test]
    fn unsuccessful_exit_overrides_claimed_completion() {
        let mut snapshot = NexusSnapshot {
            phase: "completed".into(),
            task_success: Some(true),
            ..Default::default()
        };
        finish_status(&mut snapshot, ExitStatus::from_raw(7 << 8), false);
        assert_eq!(snapshot.phase, "failed");
        assert_eq!(snapshot.task_success, None);
    }
}
