//! Studio owns a simulation session; data and commands travel through Dora nodes.
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug)]
pub struct WorkflowState {
    pub simulation: bool,
    pub active: bool,
    pub ready: bool,
    pub phase: String,
    pub detail: String,
    pub frames: u64,
    pub saved: u64,
    pub dataset: Option<PathBuf>,
    pub destination: String,
    pub task: String,
    pub calibrated: bool,
}
impl Default for WorkflowState {
    fn default() -> Self {
        Self {
            simulation: true,
            active: false,
            ready: false,
            phase: "idle".into(),
            detail: "Nano · Living room · Sofa, table and cubes · 3 cameras".into(),
            frames: 0,
            saved: 0,
            dataset: None,
            destination: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("DoRobot/Recordings")
                .display()
                .to_string(),
            task: "Pick up a cube and place it on the table".into(),
            calibrated: false,
        }
    }
}
impl WorkflowState {
    pub fn pending(&self) -> bool {
        !matches!(self.phase.as_str(), "idle" | "saved")
    }
    pub fn can_record(&self) -> bool {
        self.active && self.ready && !self.pending()
    }
}

pub struct SimulationRuntime {
    pub state: WorkflowState,
    child: Option<Child>,
    output: Option<PathBuf>,
    pub socket: Option<PathBuf>,
    poll_at: Instant,
    awaiting: Option<u64>,
    stopping: bool,
}
impl Default for SimulationRuntime {
    fn default() -> Self {
        Self {
            state: WorkflowState::default(),
            child: None,
            output: None,
            socket: None,
            poll_at: Instant::now(),
            awaiting: None,
            stopping: false,
        }
    }
}
impl SimulationRuntime {
    pub fn start(&mut self) -> Result<(), String> {
        if self.state.pending() {
            return Err(
                "Save or discard the pending take before starting a new simulation.".into(),
            );
        }
        if self.child.is_some() {
            return Err("Simulation is already running. Open Operate.".into());
        }
        let repo = crate::runtime::root();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let output = crate::runtime::runs().join(format!("studio-{stamp}"));
        let socket = PathBuf::from(format!("/tmp/dorobot-sim-{stamp}.sock"));
        let log_path = std::env::temp_dir().join(format!("dorobot-sim-{stamp}.log"));
        let log = fs::File::create(&log_path).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(crate::runtime::native());
        cmd.arg("sim")
            .arg("--model")
            .arg(repo.join("data/adora_nano/adora_nano.urdf"))
            .arg("--output")
            .arg(&output)
            .arg("--socket")
            .arg(&socket)
            .arg("--owner-pid")
            .arg(std::process::id().to_string())
            .current_dir(&repo)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        crate::runtime::configure(&mut cmd);
        self.child = Some(cmd.spawn().map_err(|e| {
            format!("Could not launch simulation: {e}. Check the runtime installation.")
        })?);
        self.output = Some(output);
        self.socket = Some(socket);
        self.awaiting = None;
        self.stopping = false;
        self.state.active = true;
        self.state.ready = false;
        self.state.phase = "idle".into();
        self.state.detail = "Starting Nano and its 3 cameras… Preparing GPU shaders.".into();
        Ok(())
    }
    pub fn stop(&mut self) -> Result<(), String> {
        if self.state.pending() {
            return Err(
                "Stop recording, then save or discard the take before stopping simulation.".into(),
            );
        }
        if let Some(root) = &self.output {
            fs::create_dir_all(root).map_err(|e| e.to_string())?;
            fs::write(root.join("stop"), b"stop").map_err(|e| e.to_string())?;
        }
        self.state.ready = false;
        self.stopping = true;
        self.state.detail = "Stopping simulation…".into();
        Ok(())
    }
    pub fn command(
        &mut self,
        action: &str,
        task: String,
        destination: String,
    ) -> Result<(), String> {
        if action == "discard" && !self.state.active {
            if let Some(root) = &self.output {
                let takes = root.join("takes");
                if takes.is_dir() {
                    fs::remove_dir_all(takes).map_err(|e| e.to_string())?;
                }
            }
            self.state.phase = "idle".into();
            self.state.frames = 0;
            self.awaiting = None;
            self.state.detail = "Unsaved take discarded. Start a new simulation.".into();
            return Ok(());
        }
        if self.awaiting.is_some() {
            return Err("Wait for the current recording operation to finish.".into());
        }
        let allowed = match action {
            "record" => self.state.can_record(),
            "stop" => self.state.phase == "recording",
            "save" => self.state.phase == "review",
            "discard" => matches!(self.state.phase.as_str(), "review" | "error"),
            _ => false,
        };
        if !allowed {
            return Err("Finish the current take before starting another operation.".into());
        }
        if task.trim().is_empty() {
            return Err("Describe the task before recording.".into());
        }
        if !PathBuf::from(destination.trim()).is_absolute() {
            return Err("Enter an absolute folder for recordings.".into());
        }
        let root = self
            .output
            .as_ref()
            .ok_or("Start simulation from Robot setup first.")?;
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let data = serde_json::json!({"id":id,"action":action,"task":task.trim(),"destination":destination.trim()});
        fs::write(root.join("session-command.tmp"), data.to_string())
            .and_then(|_| {
                fs::rename(
                    root.join("session-command.tmp"),
                    root.join("session-command.json"),
                )
            })
            .map_err(|e| e.to_string())?;
        self.state.task = task;
        self.state.destination = destination;
        self.awaiting = Some(id);
        self.state.phase = format!("{action}_pending");
        self.state.detail = match action {
            "record" => "Starting capture…",
            "stop" => "Finishing videos…",
            "save" => "Saving episode…",
            _ => "Discarding take…",
        }
        .into();
        Ok(())
    }
    pub fn poll(&mut self) {
        if self.poll_at.elapsed().as_millis() < 150 {
            return;
        }
        self.poll_at = Instant::now();
        let Some(root) = &self.output else {
            return;
        };
        if let Some(child) = &mut self.child {
            if let Ok(Some(status)) = child.try_wait() {
                self.child = None;
                self.state.active = false;
                self.state.ready = false;
                if self.state.pending() {
                    self.state.phase = "error".into();
                    self.awaiting = None;
                }
                self.state.detail = if status.success() || self.stopping {
                    "Simulation stopped. Start a new scene when ready.".into()
                } else {
                    format!("Simulation stopped ({status}). Check {} or the launcher log, then retry. Captured files are retained.",root.join("dora.log").display())
                };
                return;
            }
        }
        if !self.state.active || self.stopping {
            return;
        }
        let read = |file: &str| -> Option<serde_json::Value> {
            serde_json::from_str(&fs::read_to_string(root.join(file)).ok()?).ok()
        };
        if !self.state.ready {
            if let Some(v) = read("startup.json") {
                if let Some(message) = v["message"].as_str() {
                    self.state.detail = message.into();
                }
            }
        }
        self.state.ready =
            read("progress.json").is_some() && self.socket.as_ref().is_some_and(|p| p.exists());
        if let Some(v) = read("recording-status.json") {
            if self.awaiting.is_some() && v["request_id"].as_u64() != self.awaiting {
                return;
            }
            self.awaiting = None;
            self.state.phase = v["phase"].as_str().unwrap_or("error").into();
            self.state.frames = v["frames"].as_u64().unwrap_or(0);
            self.state.saved = v["saved"].as_u64().unwrap_or(0);
            self.state.dataset = v["dataset"].as_str().map(PathBuf::from);
            if self.state.ready {
                self.state.detail = v["message"].as_str().unwrap_or("").into();
            }
        }
    }
}
impl Drop for SimulationRuntime {
    fn drop(&mut self) {
        if let Some(root) = &self.output {
            let _ = fs::create_dir_all(root);
            let _ = fs::write(root.join("stop"), b"stop");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_requires_ready_source_and_empty_take_slot() {
        let mut state = WorkflowState::default();
        assert!(!state.can_record());
        state.active = true;
        state.ready = true;
        assert!(state.can_record());
        for phase in [
            "record_pending",
            "recording",
            "stop_pending",
            "saving",
            "review",
            "error",
            "save_pending",
        ] {
            state.phase = phase.into();
            assert!(!state.can_record(), "{phase}");
            assert!(state.pending());
        }
        state.phase = "saved".into();
        assert!(state.can_record());
        state.ready = false;
        assert!(!state.can_record());
    }
    #[test]
    fn pending_take_blocks_simulation_shutdown_and_bad_commands() {
        let mut runtime = SimulationRuntime::default();
        runtime.state.phase = "review".into();
        assert!(runtime.stop().unwrap_err().contains("save or discard"));
        assert!(runtime
            .command("record", "task".into(), "/tmp/data".into())
            .is_err());
        assert!(runtime
            .command("save", "task".into(), "relative".into())
            .unwrap_err()
            .contains("absolute"));
        assert_eq!(runtime.state.phase, "review");
    }
}
