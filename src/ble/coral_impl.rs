use async_trait::async_trait;
use thiserror::Error;
use crate::events::RecordingEvent;
use tokio::sync::mpsc;

// Blackmagic BLE Protocol Constants (from official manual)
const BLACKMAGIC_SERVICE_UUID: &str = "291D567A-6D75-11E6-8B77-86F30CA893D3";
const INCOMING_CAMERA_CONTROL_UUID: &str = "B864E140-76A0-416A-BF30-5876504537D9";
const OUTGOING_CAMERA_CONTROL_UUID: &str = "5DD3465F-1AEE-4299-8493-D2ECA2F8E1BB";
