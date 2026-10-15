use log::info;
use thiserror::Error;
// use crate::events::RecordingEvent;

#[derive(Error, Debug)]
pub enum OscError {
    #[error("OSC send failed: {0}")]
    SendFailed(String),
    #[error("Invalid OSC message: {0}")]
    InvalidMessage(String),
}

pub type OscResult<T> = Result<T, OscError>;

pub struct OscSender {
    host: String,
    port: u16,
}

impl OscSender {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    pub fn send_record_start(&self) -> OscResult<()> {
        info!("OSC: Sending /camlink/record/start to {}:{}", self.host, self.port);
        // In real implementation, use rosc here
        // rosc::udp::connect(...).send("/camlink/record/start", ...)?
        Ok(())
    }

    pub fn send_record_stop(&self, duration_ms: u64) -> OscResult<()> {
        info!("OSC: Sending /camlink/record/stop(duration={}) to {}:{}",
              duration_ms, self.host, self.port);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_osc_sender_creation() {
        let sender = OscSender::new("127.0.0.1", 6666);
        assert_eq!(sender.host, "127.0.0.1");
        assert_eq!(sender.port, 6666);
    }

    #[test]
    fn test_send_record_start_logs() {
        let sender = OscSender::new("127.0.0.1", 6666);
        let result = sender.send_record_start();
        assert!(result.is_ok());
    }
}
