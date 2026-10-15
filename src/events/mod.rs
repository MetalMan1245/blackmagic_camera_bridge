use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum RecordingEvent {
    Started { timestamp: u64 },
    Stopped { timestamp: u64, duration_ms: u64 },
    Error(String),
}

impl RecordingEvent {
    pub fn new_start() -> Self {
        Self::Started {
            timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        }
    }

    pub fn new_stop(start_time: u64) -> Self {
        let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
        Self::Stopped {
            timestamp: now,
            duration_ms: now.saturating_sub(start_time),
        }
    }
}

impl fmt::Display for RecordingEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Started { timestamp } => write!(f, "Recording started @ {}", timestamp),
            Self::Stopped { timestamp, duration_ms } => {
                write!(f, "Recording stopped @ {} (duration: {}ms)", timestamp, duration_ms)
            }
            Self::Error(msg) => write!(f, "Error: {}", msg),
        }
    }
}

mod dispatcher;

pub use dispatcher::EventDispatcher;

#[derive(Debug, Clone, PartialEq)]
pub enum RecordingEvent {
    Started { timestamp: u64 },
    Stopped { timestamp: u64, duration_ms: u64 },
    Error(String),
}

impl RecordingEvent {
    pub fn new_start() -> Self {
        Self::Started {
            timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        }
    }

    pub fn new_stop(start_time: u64) -> Self {
        let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
        Self::Stopped {
            timestamp: now,
            duration_ms: now.saturating_sub(start_time),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_start_includes_timestamp() {
        let event = RecordingEvent::new_start();
        match event {
            RecordingEvent::Started { timestamp } => assert!(timestamp > 0),
            _ => panic!("Expected Started event"),
        }
    }
}
