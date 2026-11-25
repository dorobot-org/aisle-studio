//! Supported hardware adapters are supervised Dora processes, never UI I/O.
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const NANO_LABEL: &str = "Adora Nano · Feetech base";
pub const NANO_PROFILE: &str = include_str!("../../../../robots/adora_nano_feetech/profile.json");

#[derive(Clone, Debug, Default)]
pub struct NanoState {
    pub active: bool,
    pub mock: bool,
    pub phase: String,
    pub detail: String,
    pub command_ticks: [i64; 3],
    pub acks: u64,
}

pub struct NanoRuntime {
    child: Option<Child>,
    output: Option<PathBuf>,
    last_poll: Instant,
    stopping: bool,
    pub state: NanoState,
}

impl Default for NanoRuntime {
    fn default() -> Self {
        Self {
            child: None,
            output: None,
            last_poll: Instant::now(),
            stopping: false,
            state: NanoState {
                phase: "disconnected".into(),
                detail: "Choose a USB serial port, or test with a simulated bus.".into(),
                ..Default::default()
            },
        }
    }
}

impl NanoRuntime {
    pub fn start(&mut self, mock: bool, port: &str) -> Result<(), String> {
        if self.child.is_some() {
            return Err("Disconnect the current base first.".into());
        }
        let port = port.trim();
        if !mock && (!port.starts_with("/dev/") || !std::path::Path::new(port).exists()) {
            return Err("Enter an existing USB serial device under /dev/.".into());
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let output =
            std::env::temp_dir().join(format!("dorobot-nano-base-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&output).map_err(|e| e.to_string())?;
        fs::write(output.join("lease"), b"live").map_err(|e| e.to_string())?;
        let log = fs::File::create(output.join("launcher.log")).map_err(|e| e.to_string())?;
        let mut command = Command::new(crate::runtime::native());
        command
            .arg("base")
            .arg("--output")
            .arg(&output)
            .arg("--owner-pid")
            .arg(std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        if mock {
            command.arg("--mock");
        } else {
            command.arg("--port").arg(port);
        }
        crate::runtime::configure(&mut command);
        let child = command.spawn().map_err(|e| {
            format!("Cannot start the Nano runtime: {e}. Check the runtime installation.")
        })?;
        self.child = Some(child);
        self.output = Some(output);
        self.stopping = false;
        self.state = NanoState {
            active: true,
            mock,
            phase: "starting".into(),
            detail: "Starting Dora base driver…".into(),
            ..Default::default()
        };
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(output) = &self.output {
            let _ = fs::write(output.join("stop"), b"stop");
        }
        if self.child.is_some() {
            self.stopping = true;
            self.state.detail = "Stopping wheels and disabling torque…".into();
        }
    }

    pub fn poll(&mut self) {
        if self.last_poll.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_poll = Instant::now();
        let (Some(child), Some(root)) = (&mut self.child, &self.output) else {
            return;
        };
        // A stalled/closed UI cannot leave a healthy gamepad driving indefinitely.
        let _ = fs::write(root.join("lease"), b"live");
        let packet = fs::read(root.join("status.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        if let Some(value) = packet.as_ref() {
            self.state.phase = value["phase"].as_str().unwrap_or("fault").into();
            self.state.detail = value["status"]
                .as_str()
                .unwrap_or("No driver status")
                .into();
            self.state.acks = value["acks"].as_u64().unwrap_or(0);
            for (i, v) in self.state.command_ticks.iter_mut().enumerate() {
                *v = value["command_ticks"][i].as_i64().unwrap_or(0);
            }
            let stale = fs::metadata(root.join("status.json"))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|t| t > Duration::from_secs(2));
            if stale && !self.stopping {
                let _ = fs::write(root.join("stop"), b"stop");
                self.stopping = true;
                self.state.detail = "Driver status lost · stopping".into();
            }
        }
        match child.try_wait() {
            Ok(Some(exit)) => {
                self.state.active = false;
                if !exit.success() || packet.is_none() {
                    self.state.phase = "fault".into();
                    let log = fs::read_to_string(root.join("launcher.log")).unwrap_or_default();
                    self.state.detail = log
                        .lines()
                        .rev()
                        .take(4)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect::<Vec<_>>()
                        .join("\n");
                    if self.state.detail.is_empty() {
                        self.state.detail = "Driver exited before reporting status.".into();
                    }
                } else if self.state.phase != "stopped" && self.state.phase != "fault" {
                    self.state.phase = "fault".into();
                    self.state.detail =
                        "Driver exited without confirming a stop. Check the robot connection."
                            .into();
                }
                self.child = None;
            }
            Err(e) => {
                self.state.detail = format!("Driver process: {e}");
            }
            _ => {}
        }
    }
}

impl Drop for NanoRuntime {
    fn drop(&mut self) {
        self.stop();
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_only_advertises_provided_hardware() {
        let p: serde_json::Value = serde_json::from_str(NANO_PROFILE).unwrap();
        assert_eq!(p["wheel_ids"], serde_json::json!([13, 14, 15]));
        assert_eq!(
            p["wheel_joints"],
            serde_json::json!(["wheel_2_joint", "wheel_1_joint", "wheel_3_joint"])
        );
        for capability in ["arms", "head", "cameras", "measured_odometry"] {
            assert_eq!(p["capabilities"][capability], false);
        }
    }
    #[test]
    fn physical_port_must_be_explicit_and_valid() {
        let mut runtime = NanoRuntime::default();
        assert!(runtime.start(false, "").is_err());
        assert!(runtime.start(false, "/tmp/fake-device").is_err());
        assert!(!runtime.state.active);
    }
}
