use log::info;
use rosc::encoder;
use rosc::{OscMessage, OscPacket, OscType};
use thiserror::Error;
use tokio::net::UdpSocket;

#[derive(Error, Debug)]
pub enum OscError {
    #[error("OSC send failed: {0}")]
    SendFailed(String),
    #[error("OSC encode failed: {0}")]
    EncodeFailed(String),
    #[error("Invalid OSC message: {0}")]
    InvalidMessage(String),
}

pub type OscResult<T> = Result<T, OscError>;

pub struct OscSender {
    socket: UdpSocket,
    target: String,
}

impl OscSender {
    pub async fn connect(host: &str, port: u16) -> OscResult<Self> {
        let target = format!("{}:{}", host, port);
        let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| OscError::SendFailed(format!("failed to bind UDP socket: {}", e)))?;
        socket
        .connect(&target)
        .await
        .map_err(|e| OscError::SendFailed(format!("failed to connect to {}: {}", target, e)))?;
        info!("OSC: Socket connected to {}", target);
        Ok(Self { socket, target })
    }

    async fn send(&self, addr: &str, args: Vec<OscType>) -> OscResult<()> {
        let packet = OscPacket::Message(OscMessage {
            addr: addr.to_string(),
                                        args,
        });
        let bytes = encoder::encode(&packet)
        .map_err(|e| OscError::EncodeFailed(e.to_string()))?;
        self.socket
        .send(&bytes)
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::ConnectionRefused => OscError::SendFailed(format!(
                "nothing is listening on {} - is REAPER's OSC control surface configured? \
(note: this error may refer to an earlier send)",
                                                                                  self.target
            )),
            _ => OscError::SendFailed(e.to_string()),
        })?;
        info!("OSC: Sent {} to {}", addr, self.target);
        Ok(())
    }

    pub async fn send_record_start(&self) -> OscResult<()> {
        self.send("/record", vec![OscType::Float(1.0)]).await
    }

    pub async fn send_record_stop(&self, duration_ms: u64) -> OscResult<()> {
        self.send("/stop", vec![]).await?;
        info!("OSC: Recording duration was {} ms", duration_ms);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::decoder;

    #[tokio::test]
    async fn test_send_record_start_transmits_udp_packet() {
        let listener = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let sender = OscSender::connect("127.0.0.1", port).await.unwrap();
        sender.send_record_start().await.unwrap();

        let mut buf = [0u8; 1024];
        let (len, _) = listener.recv_from(&mut buf).await.unwrap();
        let (_, packet) = decoder::decode_udp(&buf[..len]).unwrap();
        match packet {
            OscPacket::Message(msg) => {
                assert_eq!(msg.addr, "/record");
                assert_eq!(msg.args, vec![OscType::Float(1.0)]);
            }
            _ => panic!("Expected OSC message, got packet bundle"),
        }
    }

    #[tokio::test]
    async fn test_send_record_stop_transmits_udp_packet() {
        let listener = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let sender = OscSender::connect("127.0.0.1", port).await.unwrap();
        sender.send_record_stop(5000).await.unwrap();

        let mut buf = [0u8; 1024];
        let (len, _) = listener.recv_from(&mut buf).await.unwrap();
        let (_, packet) = decoder::decode_udp(&buf[..len]).unwrap();
        match packet {
            OscPacket::Message(msg) => assert_eq!(msg.addr, "/stop"),
            _ => panic!("Expected OSC message, got packet bundle"),
        }
    }
}
