//! Robot-independent live telemetry and source status.

mod receiver;
mod wire;

pub use receiver::{ReceiverError, ZenohOptions, ZenohReceiver};
pub use wire::{Descriptor, JointUnit, JointValue, PoseFrame, SourceId, MAX_PAYLOAD};

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const STALE_AFTER: Duration = Duration::from_secs(1);
const MAX_SOURCES: usize = 64;

#[derive(Debug, Clone)]
pub struct ReceivedPose {
    pub frame: PoseFrame,
    pub received_at: Instant,
    pub missed_samples: u64,
    pub receive_hz: f64,
}

#[derive(Debug, Clone)]
pub struct RemoteSource {
    pub descriptor: Descriptor,
    pub latest: Option<ReceivedPose>,
}

#[derive(Debug, Default, Clone)]
pub struct SourceCatalog {
    sources: BTreeMap<SourceId, RemoteSource>,
    generation: u64,
    pub error: Option<String>,
    pub rejected: u64,
}

impl SourceCatalog {
    pub fn sources(&self) -> impl Iterator<Item = (&SourceId, &RemoteSource)> {
        self.sources.iter()
    }

    pub fn add_source(&mut self, descriptor: Descriptor) -> bool {
        if self.sources.contains_key(&descriptor.source) || self.sources.len() >= MAX_SOURCES {
            return false;
        }
        self.sources.insert(
            descriptor.source.clone(),
            RemoteSource {
                descriptor,
                latest: None,
            },
        );
        self.generation += 1;
        true
    }

    pub fn remove_source(&mut self, source: &SourceId) {
        if self.sources.remove(source).is_some() {
            self.generation += 1;
        }
    }

    pub fn accept_pose(&mut self, frame: PoseFrame, received_at: Instant) -> bool {
        let Some(source) = self.sources.get_mut(&frame.source) else {
            return false;
        };
        if frame.model_id != source.descriptor.model_id {
            return false;
        }
        let mut missed = 0;
        let mut hz = 0.0;
        if let Some(last) = &source.latest {
            if frame.sequence <= last.frame.sequence
                || frame.capture_monotonic_ns < last.frame.capture_monotonic_ns
            {
                return false;
            }
            missed = last
                .missed_samples
                .saturating_add(frame.sequence - last.frame.sequence - 1);
            let gap = received_at
                .saturating_duration_since(last.received_at)
                .as_secs_f64();
            hz = last.receive_hz;
            if gap > 0.0 {
                let instantaneous = 1.0 / gap;
                hz = if hz > 0.0 && gap < 1.0 {
                    hz * 0.9 + instantaneous * 0.1
                } else {
                    instantaneous
                };
            }
        }
        source.latest = Some(ReceivedPose {
            frame,
            received_at,
            missed_samples: missed,
            receive_hz: hz,
        });
        self.generation += 1;
        true
    }

    pub fn latest(&self, source: &SourceId) -> Option<&ReceivedPose> {
        self.sources.get(source)?.latest.as_ref()
    }

    pub fn is_stale(&self, source: &SourceId, now: Instant) -> bool {
        self.latest(source)
            .is_none_or(|pose| now.saturating_duration_since(pose.received_at) >= STALE_AFTER)
    }

    fn report_error(&mut self, error: Option<String>) {
        self.error = error;
        self.generation += 1;
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum MirrorPhase {
    #[default]
    Searching,
    ChooseSource,
    Waiting,
    Live,
    Stale,
    Disconnected,
    UnsupportedModel,
    Error,
}

#[derive(Debug, Default)]
pub struct TelemetryState {
    pub sources: Vec<Descriptor>,
    pub selected: Option<SourceId>,
    pub latest: Option<ReceivedPose>,
    pub phase: MirrorPhase,
    pub error: Option<String>,
    pub rejected: u64,
    explicit_selection: bool,
}

impl TelemetryState {
    pub fn can_select_source(&self) -> bool {
        self.sources
            .iter()
            .any(|descriptor| Some(&descriptor.source) != self.selected.as_ref())
    }

    pub fn select(&mut self, source: SourceId) {
        if self
            .sources
            .iter()
            .any(|descriptor| descriptor.source == source)
        {
            if self.selected.as_ref() != Some(&source) {
                self.latest = None;
                self.phase = MirrorPhase::Waiting;
            }
            self.selected = Some(source);
            self.explicit_selection = true;
        }
    }

    pub fn update(
        &mut self,
        catalog: &SourceCatalog,
        now: Instant,
        supported: impl Fn(&str) -> bool,
    ) {
        self.error.clone_from(&catalog.error);
        self.rejected = catalog.rejected;
        self.sources = catalog
            .sources()
            .map(|(_, source)| source.descriptor.clone())
            .collect();
        if let Some(selected) = &self.selected {
            let matching: Vec<_> = self
                .sources
                .iter()
                .filter(|descriptor| descriptor.source.robot_id == selected.robot_id)
                .collect();
            let present = matching
                .iter()
                .any(|descriptor| &descriptor.source == selected);
            if !present && matching.len() == 1 {
                self.selected = Some(matching[0].source.clone());
                self.latest = None;
                self.explicit_selection = false;
            } else if matching.len() > 1 && (!present || !self.explicit_selection) {
                self.selected = None;
                self.latest = None;
                self.explicit_selection = false;
            }
        } else if self.sources.len() == 1 {
            self.selected = Some(self.sources[0].source.clone());
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|selected| catalog.sources.get(selected));
        self.phase = match current {
            Some(source) => {
                self.latest.clone_from(&source.latest);
                if !supported(&source.descriptor.model_id) {
                    MirrorPhase::UnsupportedModel
                } else if self.latest.is_none() {
                    MirrorPhase::Waiting
                } else if catalog.is_stale(&source.descriptor.source, now) {
                    MirrorPhase::Stale
                } else {
                    MirrorPhase::Live
                }
            }
            None if self.error.is_some() => MirrorPhase::Error,
            None if self.selected.is_some() => MirrorPhase::Disconnected,
            None if self.sources.len() > 1 => MirrorPhase::ChooseSource,
            None => MirrorPhase::Searching,
        };
    }

    pub fn render_frame(
        &self,
        source: &SourceId,
        loaded_model: &str,
        now: Instant,
    ) -> Option<&PoseFrame> {
        let pose = self.latest.as_ref()?;
        let frame = &pose.frame;
        if self.phase == MirrorPhase::Live
            && now.saturating_duration_since(pose.received_at) < STALE_AFTER
            && self.selected.as_ref() == Some(source)
            && frame.source == *source
            && frame.model_id == loaded_model
        {
            Some(frame)
        } else {
            None
        }
    }
}
