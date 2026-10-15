use async_trait::async_trait;
use thiserror::Error;
use crate::events::RecordingEvent;
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
    async fn subscribe_to_record_notifications(&mut self, tx: mpsc::Sender<RecordingEvent>) -> BleResult<()>;
    async fn trigger_record(&self, start: bool) -> BleResult<()>;
    async fn get_camera_info(&self) -> BleResult<CameraInfo>;
}

#[derive(Debug, Clone)]
pub struct CameraInfo {
    pub model: String,
    pub firmware_version: String,
    pub mac_address: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ble_error_display() {
        let err = BleError::ConnectionFailed("timeout".into());
        assert_eq!(err.to_string(), "Connection failed: timeout");
    }
}
