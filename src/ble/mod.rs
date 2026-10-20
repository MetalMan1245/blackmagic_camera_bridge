pub mod coral_impl;
mod mock;
mod packet;

pub use coral_impl::CoralCameraController;
pub use mock::MockCameraController;
pub use packet::{parse_packet, CameraPacket};

use crate::events::RecordingEvent;
use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Error, Debug)]
pub enum BleError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Pairing required: {0}")]
    PairingRequired(String),
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    #[error("Notification not available: {0}")]
    NotificationUnavailable(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type BleResult<T> = Result<T, BleError>;

#[async_trait]
pub trait CameraController: Send + Sync {
    async fn connect(&mut self, mac_address: &str) -> BleResult<()>;
    async fn disconnect(&mut self) -> BleResult<()>;
    async fn subscribe_to_record_notifications(
        &mut self,
        tx: mpsc::Sender<RecordingEvent>,
    ) -> BleResult<()>;
    async fn trigger_record(&self, start: bool) -> BleResult<()>;
    async fn get_camera_info(&self) -> BleResult<CameraInfo>;
}

#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub model: String,
    pub firmware_version: String,
    pub mac_address: String,
}

/// Connect with bounded retries and exponential backoff.
/// Backoff starts at `initial_backoff`, doubles per attempt, caps at 5 seconds.
pub async fn connect_with_retry(
    controller: &mut dyn CameraController,
    mac_address: &str,
    max_attempts: u32,
    initial_backoff: Duration,
) -> BleResult<()> {
    let mut backoff = initial_backoff.max(Duration::from_millis(10));
    let mut last_err: Option<BleError> = None;

    for attempt in 1..=max_attempts {
        match controller.connect(mac_address).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                log::warn!(
                    "Connect attempt {}/{} to {} failed: {}",
                    attempt,
                    max_attempts,
                    mac_address,
                    e
                );
                last_err = Some(e);
                if attempt < max_attempts {
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                }
            }
        }
    }

    Err(last_err.expect("at least one attempt was made"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ble_error_display() {
        let err = BleError::ConnectionFailed("timeout".into());
        assert_eq!(err.to_string(), "Connection failed: timeout");
    }

    struct FlakyCamera {
        attempts: u32,
        fail_times: u32,
    }

    #[async_trait]
    impl CameraController for FlakyCamera {
        async fn connect(&mut self, _mac_address: &str) -> BleResult<()> {
            self.attempts += 1;
            if self.attempts <= self.fail_times {
                Err(BleError::ConnectionFailed("simulated failure".into()))
            } else {
                Ok(())
            }
        }

        async fn disconnect(&mut self) -> BleResult<()> {
            Ok(())
        }

        async fn subscribe_to_record_notifications(
            &mut self,
            _tx: mpsc::Sender<crate::events::RecordingEvent>,
        ) -> BleResult<()> {
            Ok(())
        }

        async fn trigger_record(&self, _start: bool) -> BleResult<()> {
            Ok(())
        }

        async fn get_camera_info(&self) -> BleResult<CameraInfo> {
            Ok(CameraInfo {
                model: "Flaky Test Camera".into(),
                firmware_version: "0.0".into(),
                mac_address: "00:00:00:00:00:00".into(),
            })
        }
    }

    #[tokio::test]
    async fn retry_succeeds_after_transient_failures() {
        let mut cam = FlakyCamera {
            attempts: 0,
            fail_times: 2,
        };
        connect_with_retry(&mut cam, "AA:BB:CC:DD:EE:FF", 3, Duration::from_millis(10))
            .await
            .expect("should succeed on third attempt");
        assert_eq!(cam.attempts, 3);
    }

    #[tokio::test]
    async fn retry_gives_up_after_max_attempts() {
        let mut cam = FlakyCamera {
            attempts: 0,
            fail_times: 100,
        };
        let result =
            connect_with_retry(&mut cam, "AA:BB:CC:DD:EE:FF", 3, Duration::from_millis(10)).await;
        assert!(result.is_err());
        assert_eq!(cam.attempts, 3);
    }
}
