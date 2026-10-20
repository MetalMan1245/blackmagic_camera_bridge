use log::error;
use tokio::sync::mpsc;

use super::RecordingEvent;
use crate::osc::OscSender;

pub struct EventDispatcher {
    osc: OscSender,
}

impl EventDispatcher {
    pub fn new(osc: OscSender) -> Self {
        Self { osc }
    }

    pub async fn run(&self, mut rx: mpsc::Receiver<RecordingEvent>) {
        while let Some(event) = rx.recv().await {
            if let Err(e) = self.dispatch(event).await {
                error!("Failed to dispatch recording event: {}", e);
            }
        }
    }

    async fn dispatch(&self, event: RecordingEvent) -> Result<(), crate::osc::OscError> {
        match event {
            RecordingEvent::Started { .. } => self.osc.send_record_start().await,
            RecordingEvent::Stopped { duration_ms, .. } => {
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

    #[tokio::test]
    async fn test_dispatcher_sends_osc_on_started_event() {
        let listener = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let osc = OscSender::connect("127.0.0.1", port).await.unwrap();
        let dispatcher = EventDispatcher::new(osc);

        let (tx, rx) = mpsc::channel::<RecordingEvent>(10);
        tokio::spawn(async move {
            dispatcher.run(rx).await;
        });

        tx.send(RecordingEvent::Started { timestamp: 1 }).await.unwrap();

        let mut buf = [0u8; 1024];
        let (len, _) = listener.recv_from(&mut buf).await.unwrap();
        let (_, packet) = decoder::decode_udp(&buf[..len]).unwrap();
        match packet {
            OscPacket::Message(msg) => assert_eq!(msg.addr, "/record"),
            _ => panic!("Expected OSC message, got packet bundle"),
        }
    }
}
