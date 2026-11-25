use super::*;
use std::sync::mpsc::{self, Receiver, TryRecvError};

type Reply = Receiver<Result<String, String>>;

#[derive(Default)]
pub(super) struct WorkerSelection {
    pub(super) aliases: Vec<String>,
    pub(super) key: Option<(String, String, String)>,
    pub(super) ready: bool,
    pub(super) identity: String,
    probe: Option<Reply>,
    pub(super) start: Option<Reply>,
}

pub(super) fn background(mut command: Command) -> Reply {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = command.output().map_err(|e| e.to_string()).and_then(|out| {
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
            } else {
                Err(String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .last()
                    .unwrap_or("Worker check failed")
                    .to_owned())
            }
        });
        let _ = tx.send(result);
        SignalToUI::set_ui_signal();
    });
    rx
}

fn take_reply(slot: &mut Option<Reply>) -> Option<Result<String, String>> {
    let result = match slot.as_ref()?.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return None,
        Err(TryRecvError::Disconnected) => Err("Worker check interrupted".into()),
    };
    *slot = None;
    Some(result)
}

impl ExperimentPane {
    pub(super) fn worker_alias(&self) -> &str {
        self.config["worker"].as_str().unwrap_or("local")
    }

    pub(super) fn load_workers(&mut self, cx: &mut Cx) -> Result<(), String> {
        let mut aliases = vec!["local".to_string()];
        if let Some(path) = std::env::var_os("DOROBOT_AISLE_WORKERS") {
            let data = json_file(Path::new(&path))
                .map_err(|e| format!("Cannot open worker registry: {e}"))?;
            aliases.extend(
                data.as_object()
                    .ok_or("Invalid worker registry")?
                    .keys()
                    .filter(|n| *n != "local")
                    .cloned(),
            );
        }
        self.workers.aliases = aliases;
        self.show_worker_choice(cx);
        Ok(())
    }

    pub(super) fn show_worker_choice(&self, cx: &mut Cx) {
        let mut labels = self.workers.aliases.clone();
        let index = labels
            .iter()
            .position(|name| name == self.worker_alias())
            .unwrap_or_else(|| {
                labels.push(format!("{} (unregistered)", self.worker_alias()));
                labels.len() - 1
            });
        let selector = self.view.drop_down(cx, ids!(exp_worker));
        selector.set_labels(cx, labels);
        selector.set_selected_item(cx, index);
    }

    pub(super) fn update_worker(&mut self, cx: &mut Cx) {
        let alias = self.worker_alias().to_owned();
        let pinned = self.config["worker_id"].as_str().unwrap_or("").to_owned();
        let key = (alias.clone(), self.controller_sha.clone(), pinned.clone());
        let info = self.view.label(cx, ids!(exp_worker_info));
        if self.workers.key.as_ref() != Some(&key) {
            self.workers.key = Some(key);
            self.workers.probe = None;
            self.workers.ready = alias == "local";
            self.workers.identity.clear();
            if self.workers.ready {
                info.set_text(cx, "Local Nexus simulation");
            } else if let Some(path) = &self.controller {
                match self.command() {
                    Ok(mut cmd) => {
                        cmd.arg("worker").arg(root()).arg(alias).arg(path);
                        self.workers.probe = Some(background(cmd));
                        info.set_text(cx, "Checking worker and robot model…");
                    }
                    Err(err) => info.set_text(cx, &err),
                }
            } else {
                info.set_text(cx, "Prepare a controller to check worker compatibility");
            }
        }
        if let Some(result) = take_reply(&mut self.workers.probe) {
            let result = result
                .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()));
            match result {
                Ok(v) if pinned.is_empty() || v["worker_id"].as_str() == Some(pinned.as_str()) => {
                    self.workers.ready = true;
                    self.workers.identity = v["worker_id"].as_str().unwrap_or("").to_owned();
                    info.set_text(
                        cx,
                        &format!(
                            "{} · Nexus {} · {} · {} of {} slots occupied",
                            v["robot"].as_str().unwrap_or(""),
                            v["nexus"].as_str().unwrap_or(""),
                            v["backend"].as_str().unwrap_or(""),
                            v["occupied"],
                            v["slots"]
                        ),
                    );
                }
                Ok(_) => info.set_text(
                    cx,
                    "Worker identity changed. Select a worker again to save a new binding.",
                ),
                Err(err) => info.set_text(cx, &err),
            }
        }
        if let Some(result) = take_reply(&mut self.workers.start) {
            let result = result.and_then(|name| {
                let folder = PathBuf::from(name);
                self.supervise(&folder)?;
                self.batch = Some(folder);
                self.selected = 0;
                self.archives(cx).map_err(|e| e.to_string())
            });
            self.note(
                cx,
                result
                    .as_ref()
                    .err()
                    .map_or("Batch started", String::as_str),
            );
        }
    }
}
