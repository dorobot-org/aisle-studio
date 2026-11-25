use serde::Deserialize;
use std::collections::HashSet;
use thiserror::Error;

pub const PREFIX: &str = "dorobot/telemetry/v1/robots";
pub const MAX_PAYLOAD: usize = 32 * 1024;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Deserialize)]
pub struct SourceId {
    pub robot_id: String,
    pub session_id: String,
}

impl SourceId {
    pub(crate) fn from_topic(topic: &str, suffix: &str) -> Result<Self, DecodeError> {
        let tail = topic.strip_prefix(PREFIX).ok_or(DecodeError::Topic)?;
        let parts: Vec<_> = tail
            .strip_prefix('/')
            .ok_or(DecodeError::Topic)?
            .split('/')
            .collect();
        let [robot, "sessions", session, kind] = parts.as_slice() else {
            return Err(DecodeError::Topic);
        };
        if *kind != suffix || !valid_identifier(robot) || !valid_uuid(session) {
            return Err(DecodeError::Topic);
        }
        Ok(Self {
            robot_id: (*robot).into(),
            session_id: (*session).into(),
        })
    }

    pub(crate) fn topic(&self, kind: &str) -> String {
        format!(
            "{PREFIX}/{}/sessions/{}/{kind}",
            self.robot_id, self.session_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JointUnit {
    Rad,
    Deg,
    M,
    PercentOpen,
}

impl JointUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rad => "rad",
            Self::Deg => "deg",
            Self::M => "m",
            Self::PercentOpen => "% open",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct JointValue {
    pub name: String,
    pub position: f64,
    pub unit: JointUnit,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Descriptor {
    version: u32,
    #[serde(flatten)]
    pub source: SourceId,
    pub model_id: String,
    pub nominal_hz: f64,
}

impl Descriptor {
    pub fn decode(key: &str, payload: &[u8]) -> Result<Self, DecodeError> {
        let value: Self = decode(payload)?;
        validate_identity(
            key,
            "descriptor",
            value.version,
            &value.source,
            &value.model_id,
        )?;
        if !value.nominal_hz.is_finite() || value.nominal_hz <= 0.0 {
            return Err(DecodeError::Descriptor);
        }
        Ok(value)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PoseFrame {
    version: u32,
    #[serde(flatten)]
    pub source: SourceId,
    pub model_id: String,
    pub sequence: u64,
    pub capture_monotonic_ns: u64,
    pub capture_utc_ns: Option<u64>,
    pub joints: Vec<JointValue>,
}

impl PoseFrame {
    pub fn decode(key: &str, payload: &[u8]) -> Result<Self, DecodeError> {
        let value: Self = decode(payload)?;
        validate_identity(
            key,
            "joint_state",
            value.version,
            &value.source,
            &value.model_id,
        )?;
        if !(1..=128).contains(&value.joints.len()) {
            return Err(DecodeError::Joints);
        }
        let mut names = HashSet::with_capacity(value.joints.len());
        for joint in &value.joints {
            if joint.name.is_empty()
                || joint.name.len() > 128
                || !joint.position.is_finite()
                || !names.insert(joint.name.as_str())
            {
                return Err(DecodeError::Joints);
            }
        }
        Ok(value)
    }
}

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("telemetry payload exceeds 32 KiB")]
    TooLarge,
    #[error("invalid telemetry JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("telemetry topic or source identity mismatch")]
    Topic,
    #[error("unsupported telemetry version or model identifier")]
    Version,
    #[error("invalid telemetry descriptor")]
    Descriptor,
    #[error("invalid named joint positions")]
    Joints,
}

fn decode<'a, T: Deserialize<'a>>(payload: &'a [u8]) -> Result<T, DecodeError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(DecodeError::TooLarge);
    }
    Ok(serde_json::from_slice(payload)?)
}

fn validate_identity(
    key: &str,
    kind: &str,
    version: u32,
    source: &SourceId,
    model_id: &str,
) -> Result<(), DecodeError> {
    if version != 1 || !valid_identifier(model_id) {
        return Err(DecodeError::Version);
    }
    if SourceId::from_topic(key, kind)? != *source {
        return Err(DecodeError::Topic);
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, b)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
