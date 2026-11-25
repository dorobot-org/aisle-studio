//! 在后台解析模型；快速切换只接收最后一次请求，并复用最近两个未修改的模型。
use makepad_urdf_player::robot::{load_robot, Robot};
use makepad_widgets::SignalToUI;
use std::{
    collections::VecDeque,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
};

type ModelPath = (PathBuf, PathBuf);
type Request = (u64, Option<ModelPath>);
type Ready = (u64, Result<Robot, String>);

#[derive(Default)]
pub struct RobotModelLoader {
    requested: Option<ModelPath>,
    generation: u64,
    loading: bool,
    channels: Option<(Sender<Request>, Receiver<Ready>)>,
}

impl RobotModelLoader {
    pub fn request(&mut self, model: Option<ModelPath>) -> bool {
        if self.requested == model {
            return false;
        }
        self.requested = model.clone();
        self.generation += 1;
        self.loading = model.is_some();
        if self.channels.is_none() && model.is_some() {
            let (request_tx, request_rx) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::channel();
            std::thread::spawn(move || {
                worker(request_rx, ready_tx, |(model, assets)| {
                    load_robot(model, &assets.to_string_lossy()).map_err(|e| e.to_string())
                })
            });
            self.channels = Some((request_tx, ready_rx));
        }
        if let Some((tx, _)) = &self.channels {
            let _ = tx.send((self.generation, model));
        }
        true
    }

    pub fn loading(&self) -> bool {
        self.loading
    }

    pub fn take_ready(&mut self) -> Option<Result<Robot, String>> {
        let (_, rx) = self.channels.as_ref()?;
        let mut result = None;
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok((generation, ready)) if generation == self.generation && self.loading => {
                    self.loading = false;
                    result = Some(ready);
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            self.channels = None;
            self.requested = None;
            if self.loading {
                self.loading = false;
                return Some(Err(
                    "Model loading worker stopped; select a model to retry.".into(),
                ));
            }
        }
        result
    }
}

fn worker(
    requests: Receiver<Request>,
    ready: Sender<Ready>,
    load: impl Fn(&ModelPath) -> Result<Robot, String>,
) {
    let mut cache: VecDeque<(ModelPath, Option<u64>, Robot)> = VecDeque::new();
    while let Ok(mut request) = requests.recv() {
        // 当前解析结束后跳过排队的中间选择。
        for newer in requests.try_iter() {
            request = newer;
        }
        let (generation, Some(model)) = request else {
            continue;
        };
        let model = (
            model.0.canonicalize().unwrap_or(model.0),
            model.1.canonicalize().unwrap_or(model.1),
        );
        let stamp = source_stamp(&model);
        let cached = cache.iter().position(|(key, old_stamp, _)| {
            *key == model && stamp.is_some() && *old_stamp == stamp
        });
        let result = if let Some(index) = cached {
            let entry = cache.remove(index).unwrap();
            let robot = entry.2.clone();
            cache.push_back(entry);
            Ok(robot)
        } else {
            load(&model).map(|robot| {
                cache.retain(|(key, _, _)| *key != model);
                if cache.len() == 2 {
                    cache.pop_front();
                }
                cache.push_back((model, stamp, robot.clone()));
                robot
            })
        };
        if ready.send((generation, result)).is_err() {
            break;
        }
        SignalToUI::set_ui_signal();
    }
}

// URDF 和资源目录内任一文件变化都会使缓存失效；目录扫描也在后台执行。
fn source_stamp(model: &ModelPath) -> Option<u64> {
    fn visit(path: &Path, hash: &mut impl Hasher) -> std::io::Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::other("符号链接资源不使用元数据缓存"));
        }
        path.hash(hash);
        metadata.len().hash(hash);
        metadata.modified()?.hash(hash);
        if metadata.is_dir() {
            let mut children = std::fs::read_dir(path)?
                .map(|e| e.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()?;
            children.sort();
            for child in children {
                visit(&child, hash)?;
            }
        }
        Ok(())
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    visit(&model.0, &mut hash).ok()?;
    visit(&model.1, &mut hash).ok()?;
    Some(hash.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn models(tag: &str) -> (PathBuf, Vec<ModelPath>) {
        let dir = std::env::temp_dir().join(format!("dorobot-model-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let models = ["a", "b", "c"]
            .into_iter()
            .map(|name| {
                let path = dir.join(format!("{name}.urdf"));
                std::fs::write(&path, name).unwrap();
                (path, dir.clone())
            })
            .collect();
        (dir, models)
    }

    #[test]
    fn disconnected_worker_reports_an_error_and_can_be_restarted() {
        let (dir, models) = models("disconnect");
        let (tx, requests) = mpsc::channel();
        let (ready, rx) = mpsc::channel();
        let mut loader = RobotModelLoader {
            requested: Some(models[0].clone()),
            generation: 1,
            loading: true,
            channels: Some((tx, rx)),
        };
        drop(requests);
        drop(ready);
        assert!(loader.take_ready().is_some_and(|r| r.is_err()));
        assert!(!loader.loading());
        assert!(
            loader.request(Some(models[0].clone())),
            "异常退出后应能重新提交相同模型"
        );
        drop(loader);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_assets_are_not_cached_with_incomplete_metadata() {
        let (dir, models) = models("symlink");
        std::os::unix::fs::symlink(&models[0].0, dir.join("mesh.stl")).unwrap();
        assert!(source_stamp(&models[0]).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn queued_model_switches_skip_intermediate_choices() {
        let (dir, models) = models("coalesce");
        let (tx, requests) = mpsc::channel();
        let (ready, rx) = mpsc::channel();
        let (started, wait_started) = mpsc::channel();
        let (release, wait_release) = mpsc::channel();
        let loads = Arc::new(Mutex::new(Vec::new()));
        let observed = loads.clone();
        let thread = std::thread::spawn(move || {
            worker(requests, ready, |key| {
                let name = key.0.file_stem().unwrap().to_string_lossy().to_string();
                observed.lock().unwrap().push(name.clone());
                if name == "a" {
                    started.send(()).unwrap();
                    wait_release.recv().unwrap();
                }
                Ok(Robot::new(name))
            })
        });
        tx.send((1, Some(models[0].clone()))).unwrap();
        wait_started.recv_timeout(Duration::from_secs(5)).unwrap();
        // 后台尚未完成第一个加载时，界面仍能立即提交两个新选择。
        tx.send((2, Some(models[1].clone()))).unwrap();
        tx.send((3, Some(models[2].clone()))).unwrap();
        release.send(()).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap().0, 1);
        let (generation, robot) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(generation, 3);
        assert_eq!(robot.unwrap().name, "c");
        assert_eq!(*loads.lock().unwrap(), ["a", "c"]);
        drop(tx);
        thread.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cache_reuses_recent_models_and_invalidates_when_mesh_assets_change() {
        let (dir, models) = models("cache");
        let (tx, requests) = mpsc::channel();
        let (ready, rx) = mpsc::channel();
        let loads = Arc::new(Mutex::new(0));
        let observed = loads.clone();
        let thread = std::thread::spawn(move || {
            worker(requests, ready, |_| {
                *observed.lock().unwrap() += 1;
                Ok(Robot::new("loaded".into()))
            })
        });
        for (generation, index) in [(1, 0), (2, 1), (3, 0)] {
            tx.send((generation, Some(models[index].clone()))).unwrap();
            assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().1.is_ok());
        }
        assert_eq!(*loads.lock().unwrap(), 2, "来回切换不应重新解析");
        std::fs::write(dir.join("updated.stl"), "changed mesh").unwrap();
        tx.send((4, Some(models[0].clone()))).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().1.is_ok());
        assert_eq!(*loads.lock().unwrap(), 3, "模型资源修改后必须重新解析");
        drop(tx);
        thread.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cache_evicts_the_least_recent_model_without_source_changes() {
        let (dir, models) = models("capacity");
        let (tx, requests) = mpsc::channel();
        let (ready, rx) = mpsc::channel();
        let loads = Arc::new(Mutex::new(0));
        let observed = loads.clone();
        let thread = std::thread::spawn(move || {
            worker(requests, ready, |_| {
                *observed.lock().unwrap() += 1;
                Ok(Robot::new("loaded".into()))
            })
        });
        for (generation, index) in [(1, 0), (2, 1), (3, 0), (4, 2), (5, 0), (6, 1)] {
            tx.send((generation, Some(models[index].clone()))).unwrap();
            assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().1.is_ok());
        }
        assert_eq!(*loads.lock().unwrap(), 4, "缓存最多保留两个模型");
        drop(tx);
        thread.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
