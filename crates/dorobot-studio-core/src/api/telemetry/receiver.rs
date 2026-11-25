use super::{
    wire::PREFIX, Descriptor, PoseFrame, SourceCatalog, SourceId, MAX_PAYLOAD, MAX_SOURCES,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;
use zenoh::{handlers::RingChannel, sample::SampleKind, Wait};

const DESCRIPTOR_TIMEOUT: Duration = Duration::from_millis(300);
const RETRY_DELAY: Duration = Duration::from_secs(1);
type SharedCatalog = Arc<Mutex<SourceCatalog>>;

#[derive(Debug, Clone)]
pub struct ZenohOptions {
    pub mode: String,
    pub connect_endpoints: Vec<String>,
}

impl Default for ZenohOptions {
    fn default() -> Self {
        Self {
            mode: "peer".into(),
            connect_endpoints: Vec::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ReceiverError {
    #[error("Zenoh mode must be peer or client, and client mode needs a connect endpoint")]
    Config,
    #[error("could not start the Zenoh receiver: {0}")]
    Worker(#[from] std::io::Error),
}

pub struct ZenohReceiver {
    shared: SharedCatalog,
    stop: Arc<AtomicBool>,
    catalog: SourceCatalog,
}

impl ZenohReceiver {
    pub fn start(options: ZenohOptions) -> Result<Self, ReceiverError> {
        if !matches!(options.mode.as_str(), "peer" | "client")
            || (options.mode == "client" && options.connect_endpoints.is_empty())
        {
            return Err(ReceiverError::Config);
        }
        let shared = Arc::new(Mutex::new(SourceCatalog::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_catalog = shared.clone();
        let worker_stop = stop.clone();
        thread::Builder::new()
            .name("zenoh-telemetry".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Relaxed) {
                    if let Err(error) = receive(&options, &worker_catalog, &worker_stop) {
                        let mut catalog = worker_catalog.lock().unwrap();
                        catalog.sources.clear();
                        catalog.report_error(Some(error.to_string()));
                    }
                    let deadline = Instant::now() + RETRY_DELAY;
                    while Instant::now() < deadline && !worker_stop.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            })?;
        Ok(Self {
            shared,
            stop,
            catalog: SourceCatalog::default(),
        })
    }

    pub fn poll(&mut self, _now: Instant) -> &SourceCatalog {
        if let Ok(shared) = self.shared.try_lock() {
            if self.catalog.generation != shared.generation {
                self.catalog.clone_from(&shared);
            }
        }
        &self.catalog
    }
}

impl Drop for ZenohReceiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn receive(options: &ZenohOptions, shared: &SharedCatalog, stop: &AtomicBool) -> zenoh::Result<()> {
    let mut config = zenoh::Config::default();
    config.insert_json5("mode", &serde_json::to_string(&options.mode)?)?;
    config.insert_json5(
        "connect/endpoints",
        &serde_json::to_string(&options.connect_endpoints)?,
    )?;
    let multicast = options.mode == "peer" && options.connect_endpoints.is_empty();
    config.insert_json5(
        "scouting/multicast/enabled",
        if multicast { "true" } else { "false" },
    )?;
    config.insert_json5("transport/shared_memory/enabled", "false")?;
    config.insert_json5("connect/timeout_ms", "1000")?;
    let session = zenoh::open(config).wait()?;

    let poses = shared.clone();
    let _pose_subscriber = session
        .declare_subscriber(format!("{PREFIX}/*/sessions/*/joint_state"))
        .callback(move |sample| {
            let received_at = Instant::now();
            let decoded = if sample.payload().len() <= MAX_PAYLOAD {
                PoseFrame::decode(sample.key_expr().as_str(), &sample.payload().to_bytes()).ok()
            } else {
                None
            };
            let mut catalog = poses.lock().unwrap();
            if !decoded.is_some_and(|frame| catalog.accept_pose(frame, received_at)) {
                catalog.rejected = catalog.rejected.saturating_add(1);
                catalog.generation += 1;
            }
        })
        .wait()?;

    let presence = session
        .liveliness()
        .declare_subscriber(format!("{PREFIX}/*/sessions/*/alive"))
        .history(true)
        .with(RingChannel::new(MAX_SOURCES * 2))
        .wait()?;
    shared.lock().unwrap().report_error(None);

    let mut pending = BTreeMap::new();
    while !stop.load(Ordering::Relaxed) {
        if let Some(sample) = presence.recv_timeout(Duration::from_millis(25))? {
            if let Ok(source) = SourceId::from_topic(sample.key_expr().as_str(), "alive") {
                match sample.kind() {
                    SampleKind::Put if pending.len() < MAX_SOURCES => {
                        pending.entry(source).or_insert_with(Instant::now);
                    }
                    SampleKind::Delete => {
                        pending.remove(&source);
                        shared.lock().unwrap().remove_source(&source);
                    }
                    _ => {}
                }
            }
        }
        let now = Instant::now();
        if let Some(source) = pending
            .iter()
            .filter(|(_, retry)| **retry <= now)
            .min_by_key(|(_, retry)| *retry)
            .map(|(source, _)| source.clone())
        {
            if let Some(descriptor) = get_descriptor(&session, &source)? {
                shared.lock().unwrap().add_source(descriptor);
                pending.remove(&source);
            } else {
                pending.insert(source, Instant::now() + RETRY_DELAY);
            }
        }
    }
    Ok(())
}

fn get_descriptor(
    session: &zenoh::Session,
    source: &SourceId,
) -> zenoh::Result<Option<Descriptor>> {
    let key = source.topic("descriptor");
    let replies = session.get(&key).timeout(DESCRIPTOR_TIMEOUT).wait()?;
    let deadline = Instant::now() + DESCRIPTOR_TIMEOUT;
    while let Ok(Some(reply)) =
        replies.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        if let Ok(sample) = reply.result() {
            if sample.payload().len() <= MAX_PAYLOAD {
                if let Ok(descriptor) =
                    Descriptor::decode(sample.key_expr().as_str(), &sample.payload().to_bytes())
                {
                    if descriptor.source == *source {
                        return Ok(Some(descriptor));
                    }
                }
            }
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    Ok(None)
}
