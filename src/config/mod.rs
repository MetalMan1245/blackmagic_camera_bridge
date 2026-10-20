use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const BLACKMAGIC_SERVICE_UUID: &str = "291D567A-6D75-11E6-8B77-86F30CA893D3";
pub const INCOMING_CAMERA_CONTROL_UUID: &str = "B864E140-76A0-416A-BF30-5876504537D9";

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub camera: CameraConfig,
    #[serde(default)]
    pub osc: OscConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CameraConfig {
    pub mac_address: String,
    pub pairing_timeout_ms: u64,
    pub service_uuid: String,
    pub incoming_characteristic_uuid: String,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            mac_address: String::new(),
            pairing_timeout_ms: 5000,
            service_uuid: BLACKMAGIC_SERVICE_UUID.to_string(),
            incoming_characteristic_uuid: INCOMING_CAMERA_CONTROL_UUID.to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct OscConfig {
    pub target_host: String,
    pub target_port: u16,
    pub bind_port: u16,
}

impl Default for OscConfig {
    fn default() -> Self {
        Self {
            target_host: "127.0.0.1".to_string(),
            target_port: 6666,
            bind_port: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
    pub file: PathBuf,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file: PathBuf::from("/tmp/camlink.log"),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_blackmagic_protocol_uuids() {
        let config = Config::default();
        assert_eq!(
            config.camera.service_uuid,
            "291D567A-6D75-11E6-8B77-86F30CA893D3"
        );
        assert_eq!(
            config.camera.incoming_characteristic_uuid,
            "B864E140-76A0-416A-BF30-5876504537D9"
        );
    }

    #[test]
    fn partial_json_loads_with_defaults() {
        let json = r#"{
        "camera": {
        "mac_address": "AA:BB:CC:DD:EE:FF"
    }
    }"#;
        let config: Config = serde_json::from_str(json).expect("partial config should load");
        assert_eq!(config.camera.mac_address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(config.osc.target_port, 6666);
    }
}
