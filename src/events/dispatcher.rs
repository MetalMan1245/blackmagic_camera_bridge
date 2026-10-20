use log::{debug, error, info, warn};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

use super::RecordingEvent;
use crate::osc::OscSender;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub struct EventDispatcher {
    osc: OscSender,
    last_recording: Option<bool>,
}

impl EventDispatcher {
    pub fn new(osc: OscSender) -> Self {
        Self {
            osc,
            last_recording: None,
        }
    }

    pub async fn run(mut self, mut rx: mpsc::Receiver<RecordingEvent>) {
        while let Some(event) = rx.recv().await {
            if let Err(e) = self.dispatch(event).await {
                error!("Failed to dispatch recording event: {}", e);
            }
        }
    }

    async fn dispatch(&mut self, event: RecordingEvent) -> Result<(), crate::osc::OscError> {
        match event {
            RecordingEvent::Started { timestamp } => {
                if self.last_recording == Some(true) {
                    warn!("Duplicate recording-start event suppressed");
                    return Ok(());
                }
                info!("Event latency: {} ms", now_ms().saturating_sub(timestamp));
                self.last_recording = Some(true);
                self.osc.send_record_start().await
            }
            RecordingEvent::Stopped {
                timestamp,
                duration_ms,
            } => {
                if self.last_recording == Some(false) {
                    warn!("Duplicate recording-stop event suppressed");
                    return Ok(());
                }
                debug!("Event latency: {} ms", now_ms().saturating_sub(timestamp));
                self.last_recording = Some(false);
                self.osc.send_record_stop(duration_ms).await
            }
            RecordingEvent::Error(msg) => {
                error!("Recording error: {}", msg);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::{decoder, OscPacket};
    use tokio::net::UdpSocket;
    use tokio::time::{timeout, Duration};

    async fn spawn_dispatcher() -> (mpsc::Sender<RecordingEvent>, UdpSocket) {
        let listener = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let osc = OscSender::connect("127.0.0.1", port).await.unwrap();
        let dispatcher = EventDispatcher::new(osc);
        let (tx, rx) = mpsc::channel::<RecordingEvent>(10);
        tokio::spawn(async move {
            dispatcher.run(rx).await;
        });
        (tx, listener)
    }

    async fn next_addr(listener: &UdpSocket, buf: &mut [u8]) -> String {
        let (len, _) = listener.recv_from(buf).await.unwrap();
        let (_, packet) = decoder::decode_udp(&buf[..len]).unwrap();
        match packet {
            OscPacket::Message(msg) => msg.addr,
            _ => panic!("Expected OSC message, got packet bundle"),
        }
    }

    #[tokio::test]
    async fn test_dispatcher_sends_osc_on_started_event() {
        let (tx, listener) = spawn_dispatcher().await;
        let mut buf = [0u8; 1024];

        tx.send(RecordingEvent::new_start()).await.unwrap();

        let addr = next_addr(&listener, &mut buf).await;
        assert_eq!(addr, "/record");
    }

    #[tokio::test]
    async fn test_debounce_suppresses_duplicate_starts() {
        let (tx, listener) = spawn_dispatcher().await;
        let mut buf = [0u8; 1024];

        // First start
        tx.send(RecordingEvent::new_start()).await.unwrap();
        let addr1 = next_addr(&listener, &mut buf).await;
        assert_eq!(addr1, "/record");

        // Second start (duplicate) - should be suppressed
        tx.send(RecordingEvent::new_start()).await.unwrap();

        // Should not receive a second message within 2 seconds
        let result = timeout(Duration::from_secs(2), listener.recv_from(&mut buf)).await;
        assert!(
            result.is_err(),
            "Duplicate event should have been suppressed"
        );
    }

    #[tokio::test]
    async fn test_dispatcher_sends_stop_after_start() {
        let (tx, listener) = spawn_dispatcher().await;
        let mut buf = [0u8; 1024];

        let start_ts = now_ms();
        let start_event = RecordingEvent::Started {
            timestamp: start_ts,
        };
        tx.send(start_event).await.unwrap();

        let _ = next_addr(&listener, &mut buf).await; // consume /record

        let stop_event = RecordingEvent::new_stop(start_ts);
        tx.send(stop_event).await.unwrap();

        let addr = next_addr(&listener, &mut buf).await;
        assert_eq!(addr, "/stop");
    }

    #[tokio::test]
    async fn test_latency_logged_on_start() {
        let (tx, listener) = spawn_dispatcher().await;
        let mut buf = [0u8; 1024];

        let event = RecordingEvent::new_start();
        let ts = match event {
            RecordingEvent::Started { timestamp } => timestamp,
            _ => unreachable!(),
        };

        tx.send(event).await.unwrap();
        let _ = next_addr(&listener, &mut buf).await;

        // If latency exceeded 1 second, something's wrong
        let recv_ts = now_ms();
        assert!(
            recv_ts.saturating_sub(ts) < 1000,
            "Event processing took too long"
        );
    }
}
