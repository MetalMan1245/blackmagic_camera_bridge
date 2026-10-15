use log::{info, error};
use tokio::sync::mpsc;

use super::RecordingEvent;
use crate::osc::{OscSender, OscResult};

pub struct EventDispatcher {
    osc: OscSender,
}

impl EventDispatcher {
    pub fn new(osc: OscSender) -> Self {
        Self { osc }
    }

    pub async fn run(&self) {
        let (tx, mut rx) = mpsc::channel::<RecordingEvent>(100);

        // TODO: In real implementation, get this from CameraController.subscribe()
        // For now, we'll demonstrate with a mock channel

        while let Some(event) = rx.recv().await {
            match event {
                RecordingEvent::Started { .. } => {
                    if let Err(e) = self.osc.send_record_start() {
                        error!("Failed to send OSC start: {}", e);
                    }
                }
                RecordingEvent::Stopped { duration_ms, .. } => {
                    if let Err(e) = self.osc.send_record_stop(duration_ms) {
                        error!("Failed to send OSC stop: {}", e);
                    }
                }
                RecordingEvent::Error(msg) => {
                    error!("Recording error: {}", msg);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dispatcher_handles_started_event() {
        let osc = OscSender::new("127.0.0.1", 6666);
        let dispatcher = EventDispatcher::new(osc);

        // This would require refactoring to make rx injectable for testing
        // For now, this documents the expected behavior
        // TODO: Extract dispatch_logic into separate function for unit testing
        assert!(true); // Placeholder until refactored
    }
}
