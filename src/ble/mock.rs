use async_trait::async_trait;
use log::info;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

use super::*;

pub struct MockCameraController {
    connected: bool,
}

impl MockCameraController {
    pub fn new() -> Self {
        Self { connected: false }
    }
}

impl Default for MockCameraController {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CameraController for MockCameraController {
    async fn connect(&mut self, _mac_address: &str) -> BleResult<()> {
        info!("Mock: Connecting to simulated camera...");
        sleep(Duration::from_millis(50)).await;
        self.connected = true;
        Ok(())
    }

    async fn disconnect(&mut self) -> BleResult<()> {
        info!("Mock: Disconnecting from camera...");
        self.connected = false;
        Ok(())
    }

    async fn subscribe_to_record_notifications(&mut self, tx: mpsc::Sender<RecordingEvent>) -> BleResult<()> {
        info!("Mock: Simulating camera recording events...");

        // Spawn background task that emits test events
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Simulate recording start after 1 second
            sleep(Duration::from_secs(1)).await;
            let start_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
            let _ = tx_clone.send(RecordingEvent::new_start()).await;

            // Simulate recording stop after 5 seconds
            sleep(Duration::from_secs(4)).await;
            let _ = tx_clone.send(RecordingEvent::new_stop(start_ts)).await;
        });

        Ok(())
    }

    async fn trigger_record(&self, start: bool) -> BleResult<()> {
        info!("Mock: Would trigger {} record command", if start { "start" } else { "stop" });
        Ok(())
    }

    async fn get_camera_info(&self) -> BleResult<CameraInfo> {
        Ok(CameraInfo {
            model: "BMPCC 4K (Mock)".into(),
           firmware_version: "9.9.9".into(),
           mac_address: "00:00:00:00:00:00".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_connect_succeeds() {
        let mut mock = MockCameraController::new();
        let result = mock.connect("FF:FF:FF:FF:FF:FF").await;
        assert!(result.is_ok());
        assert!(mock.connected);
    }

    #[tokio::test]
    async fn test_mock_emits_events() {
        let (tx, mut rx) = mpsc::channel::<RecordingEvent>(10);
        let mut mock = MockCameraController::new();

        mock.subscribe_to_record_notifications(tx).await.unwrap();

        // Wait for first event (should arrive within 2 seconds)
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("Should receive event within timeout")
        .expect("Channel should not be closed");

        assert!(matches!(event, RecordingEvent::Started { .. }));
    }
}
