//! Local UI continuity. Restoring a session never starts playback or hardware.
use crate::api::{nexus::{NexusIntent, NexusSettings}, Backend, Intent, Screen};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub language: crate::i18n::Language,
    pub dark: bool,
    pub screen: String,
    pub dataset: Option<String>,
    pub dataset_path: Option<PathBuf>,
    pub episode: Option<u64>,
    pub time: f64,
    #[serde(default = "default_speed")]
    pub speed: f64,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f64,
    pub live_source: Option<String>,
    pub rig_profile: Option<PathBuf>,
    pub capture_task: Option<String>,
    pub capture_destination: Option<String>,
    pub simulation_source: Option<bool>,
    pub nexus: Option<NexusSettings>,
}

fn default_speed() -> f64 {
    1.0
}

fn default_sidebar_width() -> f64 {
    196.0
}

impl Session {
    pub fn snapshot(backend: &dyn Backend, dark: bool) -> Self {
        Self {
            language: crate::i18n::Language::default(),
            dark,
            screen: backend.screen().label().into(),
            dataset: backend.dataset_id().map(str::to_owned),
            dataset_path: backend
                .library()
                .datasets
                .iter()
                .find(|d| Some(d.id.as_str()) == backend.dataset_id())
                .and_then(|d| d.path.canonicalize().ok()),
            episode: backend.replay().selected,
            time: backend.replay().current_time,
            speed: backend.replay().speed,
            sidebar_width: default_sidebar_width(),
            live_source: backend.live_connection().map(|c| c.source.clone()),
            rig_profile: backend.rig_profile(),
            capture_task: Some(backend.workflow().task),
            capture_destination: Some(backend.workflow().destination),
            simulation_source: Some(backend.workflow().simulation),
            nexus: backend.nexus().map(|state| state.settings.clone()),
        }
    }

    pub fn restore(&self, backend: &mut dyn Backend, restore_dataset: bool) {
        if let (Some(mut settings), Some(state)) = (self.nexus.clone(), backend.nexus()) {
            if std::env::var_os("DOROBOT_LEROBOT_PYTHON").is_some() {
                settings.python.clone_from(&state.settings.python);
            }
            if std::env::var_os("DOROBOT_SO101_LEADER").is_some() {
                settings.leader_port.clone_from(&state.settings.leader_port);
            }
            if std::env::var_os("DOROBOT_SO101_CALIBRATION").is_some() {
                settings.calibration_dir.clone_from(&state.settings.calibration_dir);
            }
            backend.dispatch(Intent::Nexus(NexusIntent::Configure(settings)));
        }
        if let Some(path) = &self.rig_profile { if path.is_file() { backend.dispatch(Intent::ConfigureRig(path.clone())); } }
        if let Some(selected) = self.simulation_source { backend.dispatch(Intent::SelectSimulation(selected)); }
        let flow = backend.workflow();
        backend.dispatch(Intent::SetCapturePreferences {task:self.capture_task.clone().unwrap_or(flow.task),destination:self.capture_destination.clone().unwrap_or(flow.destination)});
        if let Some(source) = &self.live_source { backend.dispatch(Intent::SetLiveSource(source.clone())); }
        if restore_dataset {
            if let Some(path) = &self.dataset_path {
                if path.is_dir() {
                    backend.dispatch(Intent::OpenFolder(path.clone()));
                }
            }
            if let Some(id) = &self.dataset {
                if backend.library().datasets.iter().any(|d| &d.id == id) {
                    backend.dispatch(Intent::OpenDataset(id.clone()));
                }
            }
        }
        // Never apply the old playhead to an explicitly opened, different dataset.
        let same_path = self.dataset_path.as_ref().is_none_or(|saved| {
            backend.library().datasets.iter().any(|d| {
                Some(d.id.as_str()) == backend.dataset_id()
                    && d.path.canonicalize().ok().as_ref() == Some(saved)
            })
        });
        if same_path && backend.dataset_id() == self.dataset.as_deref() && self.dataset.is_some() {
            if let Some(episode) = self.episode {
                if backend
                    .playback()
                    .episodes
                    .iter()
                    .any(|e| e.index == episode)
                {
                    backend.dispatch(Intent::SelectEpisode(episode));
                    if self.time.is_finite() {
                        backend.dispatch(Intent::Seek(self.time));
                        if self.speed.is_finite() && self.speed > 0.0 {
                            backend.dispatch(Intent::SetSpeed(self.speed));
                        }
                    }
                }
            }
        }
        if let Some(screen) = Screen::ALL.into_iter().find(|s| s.label() == self.screen || matches!((s,self.screen.as_str()), (Screen::Hardware,"Hardware") | (Screen::Live,"Live") | (Screen::Play,"Play"))) {
            backend.dispatch(Intent::Navigate(screen));
        }
    }
}

pub fn path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DOROBOT_STUDIO_STATE_DIR") {
        return Some(PathBuf::from(dir).join("session.json"));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    #[cfg(target_os = "macos")]
    let dir = home.join("Library/Application Support/DoRobot Studio");
    #[cfg(not(target_os = "macos"))]
    let dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"))
        .join("dorobot-studio");
    Some(dir.join("session.json"))
}

pub fn load(path: &Path) -> Option<Session> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn save(path: &Path, session: &Session) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(session)?;
    if std::fs::read(path).is_ok_and(|previous| previous == bytes) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Distinct temporary files let separate app instances save without sharing a writer.
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_survives_restart_and_ignores_corrupt_state() {
        let root = std::env::temp_dir().join(format!("dorobot-ui-session-{}", std::process::id()));
        let path = root.join("session.json");
        let session = Session {
            dark: true,
            screen: "Play".into(),
            dataset: Some("sample".into()),
            dataset_path: None,
            episode: Some(2),
            time: 1.5,
            speed: 2.0,
            sidebar_width: 224.0,
            live_source: None,
            ..Default::default()
        };
        save(&path, &session).unwrap();
        assert_eq!(load(&path), Some(session));
        std::fs::write(&path, b"interrupted").unwrap();
        assert!(load(&path).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
