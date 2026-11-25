//! Local Dora preview transport. A reader thread replaces a single latest-frame
//! slot; the UI never blocks a simulator or shares the recorder's queue.
use super::{
    scene::{BasePose, SceneFrame, SceneObject, SceneReplay},
    *,
};
use serde::Deserialize;
use std::{
    collections::VecDeque,
    io::{self, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

/// Reconnect the current local controller simulation without baking a run's
/// temporary socket name into the UI. Ignore recordings and stale runs.
pub fn nano_gamepad_source() -> PathBuf {
    let runs = crate::runtime::runs();
    find_gamepad_source(&runs).unwrap_or_else(|| PathBuf::from("/tmp/nano-gamepad.sock"))
}

fn find_gamepad_source(runs: &std::path::Path) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        std::fs::read_dir(runs).ok()?.flatten().filter_map(|entry| {
            let root = entry.path();
            let modified = std::fs::metadata(root.join("progress.json")).ok()?.modified().ok()?;
            if modified.elapsed().ok()? > Duration::from_secs(5) { return None; }
            let info: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("scene-info.json")).ok()?).ok()?;
            if !info["session"].as_str()?.starts_with("Nano gamepad") || info["mode"] != "simulation" { return None; }
            let config: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("config.json")).ok()?).ok()?;
            let socket = PathBuf::from(config["socket"].as_str()?);
            if !socket.is_absolute() || !std::fs::metadata(&socket).ok()?.file_type().is_socket() { return None; }
            Some((modified, socket))
        }).max_by_key(|(modified, _)| *modified).map(|(_, socket)| socket)
    }
    #[cfg(not(unix))]
    { let _ = runs; None }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionPhase {
    #[default]
    Disconnected,
    Connecting,
    Streaming,
    Waiting,
    Completed,
    Error,
}

impl ConnectionPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Disconnected",
            Self::Connecting => "Connecting",
            Self::Streaming => "Live",
            Self::Waiting => "Waiting for stream",
            Self::Completed => "Stream ended",
            Self::Error => "Cannot connect",
        }
    }
    pub fn active(self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::Streaming | Self::Waiting | Self::Completed
        )
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionState {
    pub source: String,
    pub phase: ConnectionPhase,
    /// Actual preview samples accepted by the UI per wall-clock second.
    pub preview_fps: f64,
    pub frame_age_ms: Option<u64>,
    pub received_frames: u64,
    pub dataset: Option<PathBuf>,
    pub detail: String,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            source: "/tmp/nano-living.sock".into(),
            phase: ConnectionPhase::Disconnected,
            preview_fps: 0.,
            frame_age_ms: None,
            received_frames: 0,
            dataset: None,
            detail: "Connect a local Dora feed to view cameras and robot motion.".into(),
        }
    }
}

impl ConnectionState {
    pub fn summary(&self) -> String {
        if self.phase == ConnectionPhase::Streaming {
            if let Some(mode) = self.detail.split_once(" · Gamepad · ").map(|(_,mode)|mode) {
                return format!("LIVE · Simulation · {} · {} · {:.1} fps preview", mode, if self.detail.starts_with("Recording") { "● Recording" } else { "Recording off" }, self.preview_fps);
            }
            format!(
                "LIVE · {}{:.1} fps preview · 3 cameras · last frame {} ms ago",
                if self.detail.starts_with("Preview only") { "Recording off · " } else { "" },
                self.preview_fps,
                self.frame_age_ms.unwrap_or(0)
            )
        } else {
            format!("{} · {}", self.phase.label(), self.detail)
        }
    }
}

pub fn empty_preview() -> PlaybackState {
    PlaybackState {
        dataset_name: "Live preview".into(),
        live: Some(LiveView {
            sequence: 0,
            images: BTreeMap::new(),
            connected: false,
            finished: false,
        }),
        ..Default::default()
    }
}

#[derive(Clone, Debug)]
pub struct LiveImage {
    pub width: usize,
    pub height: usize,
    pub bgra: Arc<Vec<u32>>,
}
#[derive(Clone, Debug)]
pub struct LiveView {
    pub sequence: u64,
    pub images: BTreeMap<String, LiveImage>,
    pub connected: bool,
    pub finished: bool,
}
#[derive(Debug, Deserialize)]
pub struct CameraHeader {
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub bytes: usize,
}
#[derive(Debug, Deserialize)]
pub struct Header {
    pub version: u32,
    pub session: String,
    pub sequence: u64,
    pub sim_time_ns: u64,
    pub fps: f64,
    pub joint_names: Vec<String>,
    pub joints: Vec<f32>,
    pub targets: Vec<f32>,
    pub base_position: [f32; 3],
    pub base_quat_wxyz: [f32; 4],
    pub objects: Vec<SceneObject>,
    pub transforms: Vec<[f32; 16]>,
    pub cameras: Vec<CameraHeader>,
    pub phase: String,
    #[serde(default)]
    pub finished: bool,
    #[serde(default)]
    pub dataset: Option<PathBuf>,
}
#[derive(Debug)]
pub struct Packet {
    pub header: Header,
    pub images: BTreeMap<String, LiveImage>,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn connection_error(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "Source is not running yet.".into(),
        io::ErrorKind::ConnectionRefused => "Source is not accepting connections yet.".into(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "No new frames received.".into(),
        io::ErrorKind::UnexpectedEof => "Source closed the connection.".into(),
        io::ErrorKind::PermissionDenied => "Cannot access this source.".into(),
        _ => error.to_string(),
    }
}

pub fn read_packet(reader: &mut impl Read) -> io::Result<Packet> {
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size == 0 || size > 2_000_000 {
        return Err(invalid("Invalid preview header length"));
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes)?;
    let header: Header =
        serde_json::from_slice(&bytes).map_err(|_| invalid("Invalid preview metadata"))?;
    if header.version != 1
        || header.joint_names.len() != 17
        || header.joints.len() != 17
        || header.targets.len() != 17
        || !(1.0..=60.0).contains(&header.fps)
        || header.objects.is_empty()
        || header.objects.len() > 512
        || header.objects.len() != header.transforms.len()
        || header.cameras.len() != 3
        || !header
            .joints
            .iter()
            .chain(&header.targets)
            .chain(&header.base_position)
            .chain(&header.base_quat_wxyz)
            .all(|v| v.is_finite())
        || header.base_quat_wxyz.iter().map(|v| v * v).sum::<f32>() < 0.5
    {
        return Err(invalid("Invalid synchronized preview frame"));
    }
    let timestamp = header.sim_time_ns as f64 * 1e-9;
    let scene = SceneReplay {
        version: 2,
        objects: header.objects.clone(),
        frames: vec![SceneFrame {
            timestamp,
            sim_time_ns: header.sim_time_ns as i64,
            transforms: header.transforms.clone(),
            base_pose: Some(BasePose {
                position: header.base_position,
                quat_xyzw: [
                    header.base_quat_wxyz[1],
                    header.base_quat_wxyz[2],
                    header.base_quat_wxyz[3],
                    header.base_quat_wxyz[0],
                ],
            }),
        }],
    };
    if !scene.validate(&[timestamp]) {
        return Err(invalid("Invalid preview scene geometry"));
    }
    let mut images = BTreeMap::new();
    for camera in &header.cameras {
        if !["head", "left_wrist", "right_wrist"].contains(&camera.name.as_str())
            || camera.width == 0
            || camera.height == 0
            || camera.width > 1920
            || camera.height > 1080
            || camera.bytes != camera.width * camera.height * 4
            || images.contains_key(&camera.name)
        {
            return Err(invalid("Invalid preview camera dimensions or identity"));
        }
        let mut bgra = vec![0_u32; camera.width * camera.height];
        // The initialized u32 allocation is aligned and its byte view has
        // exactly the checked camera length. Read straight into texture data.
        let bytes =
            unsafe { std::slice::from_raw_parts_mut(bgra.as_mut_ptr().cast::<u8>(), camera.bytes) };
        reader.read_exact(bytes)?;
        images.insert(
            camera.name.clone(),
            LiveImage {
                width: camera.width,
                height: camera.height,
                bgra: Arc::new(bgra),
            },
        );
    }
    Ok(Packet { header, images })
}

pub struct LiveClient {
    latest: Arc<Mutex<Option<(Packet, Instant)>>>,
    stopped: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    arrivals: VecDeque<Instant>,
    pub received_frames: u64,
    pub last_frame: Instant,
    pub sequence: Option<u64>,
    session: Option<String>,
}
impl LiveClient {
    pub fn connect(path: PathBuf) -> Self {
        let latest = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let errors = error.clone();
        let (slot, stop) = (latest.clone(), stopped.clone());
        thread::spawn(move || {
            #[cfg(unix)]
            while !stop.load(Ordering::Relaxed) {
                match std::os::unix::net::UnixStream::connect(&path) {
                    Ok(mut stream) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        while !stop.load(Ordering::Relaxed) {
                            match read_packet(&mut stream) {
                                Ok(packet) => {
                                    if let Ok(mut latest) = slot.lock() {
                                        *latest = Some((packet, Instant::now()));
                                    }
                                    if let Ok(mut error) = errors.lock() {
                                        *error = None;
                                    }
                                }
                                Err(e) => {
                                    if let Ok(mut error) = errors.lock() {
                                        *error = Some(connection_error(&e));
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if let Ok(mut error) = errors.lock() {
                            *error = Some(connection_error(&e));
                        }
                    }
                }
                thread::sleep(Duration::from_millis(250));
            }
        });
        Self {
            latest,
            stopped,
            error,
            arrivals: VecDeque::new(),
            received_frames: 0,
            last_frame: Instant::now(),
            sequence: None,
            session: None,
        }
    }
    pub fn take(&mut self) -> Option<Packet> {
        let (packet, received_at) = self.latest.lock().ok()?.take()?;
        if self.session.as_deref() == Some(packet.header.session.as_str())
            && self.sequence.is_some_and(|s| packet.header.sequence <= s)
        {
            return None;
        }
        if self.session.as_deref() != Some(packet.header.session.as_str()) {
            self.arrivals.clear();
            self.received_frames = 0;
        }
        self.session = Some(packet.header.session.clone());
        self.sequence = Some(packet.header.sequence);
        self.last_frame = received_at;
        if !packet.header.finished {
            self.received_frames += 1;
            self.arrivals.push_back(received_at);
            while self
                .arrivals
                .front()
                .is_some_and(|at| received_at.duration_since(*at) > Duration::from_secs(2))
            {
                self.arrivals.pop_front();
            }
        }
        Some(packet)
    }
    pub fn preview_fps(&self) -> f64 {
        if self.stale() || self.arrivals.len() < 2 {
            return 0.;
        }
        let span = self
            .arrivals
            .back()
            .unwrap()
            .duration_since(*self.arrivals.front().unwrap())
            .as_secs_f64();
        if span > 0. {
            (self.arrivals.len() - 1) as f64 / span
        } else {
            0.
        }
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok()?.clone()
    }
    pub fn stale(&self) -> bool {
        self.last_frame.elapsed() > Duration::from_secs(3)
    }
}
impl Drop for LiveClient {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

pub fn apply_packet(state: &mut PlaybackState, packet: Packet, model: (PathBuf, PathBuf)) {
    let h = packet.header;
    if state.dataset_name != h.session {
        *state = PlaybackState {
            dataset_name: h.session.clone(),
            selected: Some(0),
            episodes: vec![EpisodeEntry {
                index: 0,
                task_group: "Living room navigation".into(),
                duration_s: 0.,
                tag: None,
            }],
            robot_urdf: Some(model),
            speed: 1.,
            state_names: h.joint_names.clone(),
            action_names: h.joint_names.clone(),
            scene: Some(SceneReplay {
                version: 2,
                objects: h.objects.clone(),
                frames: Vec::new(),
            }),
            ..Default::default()
        };
        state.state_series = h
            .joint_names
            .iter()
            .map(|n| PlotChannel {
                name: n.clone(),
                points: Vec::new(),
            })
            .collect();
        state.action_series = state.state_series.clone();
    }
    let t = h.sim_time_ns as f64 * 1e-9;
    for (i, channel) in state.state_series.iter_mut().enumerate() {
        channel.points.push((t, h.joints[i] as f64));
    }
    for (i, channel) in state.action_series.iter_mut().enumerate() {
        channel.points.push((t, h.targets[i] as f64));
    }
    state.joint_frames.push(h.joints);
    if let Some(scene) = &mut state.scene {
        scene.frames.push(SceneFrame {
            timestamp: t,
            sim_time_ns: h.sim_time_ns as i64,
            transforms: h.transforms,
            base_pose: Some(BasePose {
                position: h.base_position,
                quat_xyzw: [
                    h.base_quat_wxyz[1],
                    h.base_quat_wxyz[2],
                    h.base_quat_wxyz[3],
                    h.base_quat_wxyz[0],
                ],
            }),
        });
    }
    // Bound live history independently of the lossless on-disk recording.
    if state.joint_frames.len() > 1200 {
        state.joint_frames.remove(0);
        if let Some(s) = &mut state.scene {
            s.frames.remove(0);
        }
        for c in state
            .state_series
            .iter_mut()
            .chain(&mut state.action_series)
        {
            c.points.remove(0);
        }
    }
    state.current_time = t;
    state.stats = EpisodeStats {
        frames: h.sequence + 1,
        duration_s: t.max(1. / h.fps),
        fps: h.fps,
        task: h.phase,
        state_channels: 17,
        action_channels: 17,
        ..Default::default()
    };
    state.episodes[0].duration_s = t;
    state.live = Some(LiveView {
        sequence: h.sequence,
        images: packet.images,
        connected: true,
        finished: h.finished,
    });
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn discovers_only_active_gamepad_simulation_sockets() {
        use std::{fs, os::unix::net::UnixListener};
        let root = std::env::temp_dir().join(format!("nano-source-test-{}", std::process::id()));
        let run = root.join("run");
        fs::create_dir_all(&run).unwrap();
        let socket = root.join("feed.sock");
        let _ = fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        fs::write(run.join("config.json"), serde_json::json!({"socket":socket}).to_string()).unwrap();
        fs::write(run.join("progress.json"), "{}").unwrap();
        fs::write(run.join("scene-info.json"), r#"{"session":"Nano gamepad · test","mode":"simulation"}"#).unwrap();
        assert_eq!(super::find_gamepad_source(&root), Some(socket.clone()));
        fs::write(run.join("scene-info.json"), r#"{"session":"Nano gamepad · test","mode":"hardware"}"#).unwrap();
        assert!(super::find_gamepad_source(&root).is_none());
        fs::write(run.join("scene-info.json"), r#"{"session":"Nano gamepad · test","mode":"simulation"}"#).unwrap();
        drop(listener);
        fs::remove_file(&socket).unwrap();
        fs::write(&socket, "not a socket").unwrap();
        assert!(super::find_gamepad_source(&root).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    use super::*;
    fn wire(sequence: u64) -> Vec<u8> {
        let h = serde_json::json!({"version":1,"session":"test-live","sequence":sequence,"sim_time_ns":(sequence+1)*200_000_000_u64,"fps":5.,"joint_names":(0..17).map(|i|format!("joint_{i}")).collect::<Vec<_>>(),"joints":vec![sequence as f32;17],"targets":vec![0.;17],"base_position":[sequence as f32,0,0],"base_quat_wxyz":[1,0,0,0],"objects":[{"name":"cube","size":[0.1,0.1,0.1],"color":[1,0,0,1]}],"transforms":[[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]],"cameras":[{"name":"head","width":2,"height":2,"bytes":16},{"name":"left_wrist","width":2,"height":2,"bytes":16},{"name":"right_wrist","width":2,"height":2,"bytes":16}],"phase":"navigation"});
        let data = serde_json::to_vec(&h).unwrap();
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend(data);
        out.extend(vec![sequence as u8; 48]);
        out
    }
    #[test]
    fn dropped_preview_frames_keep_pose_images_and_cursor_on_the_same_sample() {
        let mut state = PlaybackState::default();
        let model = ("nano.urdf".into(), "assets".into());
        for seq in [0, 4] {
            let packet = read_packet(&mut wire(seq).as_slice()).unwrap();
            apply_packet(&mut state, packet, model.clone());
        }
        assert_eq!(state.frame_index(), 1);
        assert_eq!(state.joints_now().unwrap()[0], 4.);
        assert_eq!(
            state.scene.as_ref().unwrap().frames[1]
                .base_pose
                .as_ref()
                .unwrap()
                .position[0],
            4.
        );
        assert_eq!(
            state.live.as_ref().unwrap().images["head"].bgra[0],
            0x04040404
        );
        assert_eq!(state.current_time, 1.);
        assert_eq!(state.stats.frames, 5);
    }
    #[test]
    fn incomplete_or_oversized_packets_are_never_published() {
        let mut bytes = wire(0);
        bytes.pop();
        assert!(read_packet(&mut bytes.as_slice()).is_err());
        assert!(read_packet(&mut (2_000_001_u32).to_be_bytes().as_slice()).is_err());
    }
    #[test]
    fn reconnect_accepts_a_new_session_and_rejects_old_samples() {
        let mut client = LiveClient {
            latest: Arc::new(Mutex::new(None)),
            stopped: Arc::new(AtomicBool::new(false)),
            error: Arc::new(Mutex::new(None)),
            arrivals: VecDeque::new(),
            received_frames: 0,
            last_frame: Instant::now(),
            sequence: Some(20),
            session: Some("test-live".into()),
        };
        *client.latest.lock().unwrap() = Some((
            read_packet(&mut wire(0).as_slice()).unwrap(),
            Instant::now(),
        ));
        assert!(client.take().is_none());
        let mut packet = read_packet(&mut wire(0).as_slice()).unwrap();
        packet.header.session = "second-run".into();
        *client.latest.lock().unwrap() = Some((packet, Instant::now()));
        assert_eq!(client.take().unwrap().header.sequence, 0);
        assert_eq!(client.session.as_deref(), Some("second-run"));
    }

    #[test]
    fn preview_rate_counts_received_samples_not_source_sequence_gaps() {
        let now = Instant::now();
        let mut client = LiveClient {
            latest: Arc::new(Mutex::new(None)),
            stopped: Arc::new(AtomicBool::new(false)),
            error: Arc::new(Mutex::new(None)),
            arrivals: VecDeque::new(),
            received_frames: 0,
            last_frame: now,
            sequence: None,
            session: None,
        };
        for (sequence, age) in [(0, 200), (30, 0)] {
            *client.latest.lock().unwrap() = Some((
                read_packet(&mut wire(sequence).as_slice()).unwrap(),
                now - Duration::from_millis(age),
            ));
            client.take().unwrap();
        }
        assert!((client.preview_fps() - 5.).abs() < 1e-9);
        assert_eq!(client.received_frames, 2);
        client.last_frame = now - Duration::from_secs(4);
        assert!(client.stale());
        assert_eq!(client.preview_fps(), 0.);
    }

    #[test]
    #[cfg(unix)]
    fn live_socket_keeps_replay_separate_and_completion_does_not_add_a_frame() {
        use crate::api::files::FileBackend;
        use std::{io::Write, os::unix::net::UnixListener, sync::mpsc};
        let root = std::env::temp_dir().join(format!("dlive-{}", std::process::id()));
        std::fs::create_dir_all(root.join("recording/meta")).unwrap();
        std::fs::write(root.join("recording/meta/info.json"), b"{}").unwrap();
        let socket = root.join("feed.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (advance, gate) = mpsc::channel();
        let dataset = root.join("recording");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(&wire(0)).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            stream.write_all(&wire(3)).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            let bytes = wire(4);
            let len = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
            let mut header: serde_json::Value = serde_json::from_slice(&bytes[4..4 + len]).unwrap();
            header["finished"] = true.into();
            header["sim_time_ns"] = 800_000_000_u64.into();
            header["dataset"] = dataset.to_string_lossy().to_string().into();
            let json = serde_json::to_vec(&header).unwrap();
            stream
                .write_all(&(json.len() as u32).to_be_bytes())
                .unwrap();
            stream.write_all(&json).unwrap();
            stream.write_all(&bytes[4 + len..]).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let mut backend = FileBackend::new(root.join("empty"));
        backend.dispatch(Intent::ConnectLive(socket));
        fn until(backend: &mut FileBackend, done: impl Fn(&FileBackend) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                backend.poll();
                if done(backend) {
                    break;
                }
                assert!(Instant::now() < deadline, "live transition timed out");
                thread::sleep(Duration::from_millis(5));
            }
        }
        until(&mut backend, |b| {
            b.live_connection().unwrap().received_frames == 1
        });
        assert_eq!(backend.screen(), Screen::Live);
        assert_eq!(backend.playback().live.as_ref().unwrap().images.len(), 3);
        backend.dispatch(Intent::Navigate(Screen::Play));
        advance.send(()).unwrap();
        until(&mut backend, |b| {
            b.live_connection().unwrap().received_frames == 2
        });
        assert!(backend.playback().live.is_none());
        assert_eq!(backend.playback().current_time, 0.);
        backend.dispatch(Intent::Navigate(Screen::Live));
        assert_eq!(backend.playback().joints_now().unwrap()[0], 3.);
        advance.send(()).unwrap();
        until(&mut backend, |b| {
            b.live_connection().unwrap().phase == ConnectionPhase::Completed
        });
        assert_eq!(backend.screen(), Screen::Live);
        assert_eq!(backend.playback().stats.frames, 4);
        assert_eq!(backend.playback().current_time, 0.8);
        assert_eq!(backend.live_connection().unwrap().received_frames, 2);
        assert!(backend.live_connection().unwrap().dataset.is_some());
        backend.dispatch(Intent::DisconnectLive);
        assert_eq!(
            backend.live_connection().unwrap().phase,
            ConnectionPhase::Disconnected
        );
        advance.send(()).unwrap();
        server.join().unwrap();
        drop(backend);
        std::fs::remove_dir_all(root).unwrap();
    }
}
