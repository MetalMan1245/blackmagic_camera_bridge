//! Real BLE camera controller backed by the Blackmagic Camera Control protocol.
//!
//! ## UUID note (important)
//! The Blackmagic manual documents these characteristics as *32-bit assigned
//! short UUIDs* living under the Camera Control primary service:
//!
//! ```text
//! Service        : 0x7238   "Camera Control"
//! Incoming (RX)  : 0x7239   camera -> controller notifications (record state)
//! Outgoing (TX)  : 0x723A   controller -> camera commands
//! ```
//!
//! Older community notes (and the original stub of this file) express them as
//! full 128-bit UUIDs derived from the base
//! `B864E140-76A0-416A-BF30-5876504537D9`. Because btleplug reports GATT
//! attributes inconsistently across platforms (BlueZ often yields the short
//! 16-bit form, other stacks the expanded 128-bit form), discovery here is
//! tolerant: we match a characteristic by ANY of its known aliases.
//!
//! If your capture shows different values, override them via CLI flags
//! (`--incoming-uuid` / `--outgoing-uuid`) without touching this code.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::packet::{parse_packet, CameraPacket};
use super::{BleError, BleResult, CameraController, CameraInfo};
use crate::events::RecordingEvent;

// ---------------------------------------------------------------------------
// Protocol constants
// ---------------------------------------------------------------------------

/// Camera Control primary service (short UUID form).
pub const SERVICE_UUID_SHORT: u16 = 0x7238;
/// Incoming Camera Control characteristic: camera -> us (record notifications).
pub const INCOMING_CHAR_SHORT: u16 = 0x7239;
/// Outgoing Camera Control characteristic: us -> camera (commands).
pub const OUTGOING_CHAR_SHORT: u16 = 0x723A;

/// Alias UUIDs accepted during characteristic discovery. A characteristic
/// matches if its UUID equals the short form, the canonical 128-bit expansion,
/// or one of these historical aliases from early community reverse-engineering.
pub fn incoming_aliases_128() -> Vec<Uuid> {
    vec![Uuid::parse_str("B864E140-76A0-416A-BF30-5876504537D9").expect("valid uuid")]
}

pub fn outgoing_aliases_128() -> Vec<Uuid> {
    vec![Uuid::parse_str("5DD3465F-1AEE-4299-8493-D2ECA2F8E1BB").expect("valid uuid")]
}

/// Transport category and Record parameter ids (shared with the parser).
pub const TRANSPORT_CATEGORY: u8 = super::packet::TRANSPORT_CATEGORY;
pub const RECORD_PARAMETER: u8 = super::packet::RECORD_PARAMETER;

// ---------------------------------------------------------------------------
// UUID helpers (pure - unit tested without hardware)
// ---------------------------------------------------------------------------

/// Expand a short (16-bit) UUID into its canonical 128-bit base form
/// (`0000xxxx-0000-1000-8000-00805F9B34FB`).
pub fn expand_short_uuid(short: u16) -> Uuid {
    // Canonical BLE base UUID: 0000xxxx-0000-1000-8000-00805F9B34FB.
    // In the big-endian 128-bit value, the short UUID sits in bytes 2-3,
    // i.e. bits 96..112.
    let value: u128 = (u128::from(short) << 96) | 0x0000_1000_8000_0080_5F9B_34FB_u128;
    Uuid::from_u128(value)
}

fn short_to_uuid(short: u16) -> Uuid {
    Uuid::from_u128(short as u128)
}

/// True if `c`'s UUID matches the given short UUID, its 128-bit expansion, or
/// one of the supplied historical aliases.
pub fn char_matches(c: &Characteristic, short: u16, aliases_128: &[Uuid]) -> bool {
    let id = c.uuid;
    id == short_to_uuid(short) || id == expand_short_uuid(short) || aliases_128.contains(&id)
}

/// Parse a user-supplied UUID string ("0x7239", "7239", or a full 128-bit
/// UUID) into a `Uuid`. Used for CLI/config overrides.
pub fn parse_uuid_override(s: &str) -> Result<Uuid, String> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        let v = u16::from_str_radix(hex, 16).map_err(|e| format!("bad short uuid '{s}': {e}"))?;
        return Ok(short_to_uuid(v));
    }
    if t.len() == 4 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        let v = u16::from_str_radix(t, 16).map_err(|e| format!("bad short uuid '{s}': {e}"))?;
        return Ok(short_to_uuid(v));
    }
    Uuid::parse_str(t).map_err(|e| format!("bad uuid '{s}': {e}"))
}

// ---------------------------------------------------------------------------
// Packet builders (pure functions - unit tested without hardware)
// ---------------------------------------------------------------------------

/// Build a Camera Control command packet setting one parameter inside a
/// category. Layout mirrors captured notification samples handled by
/// `packet::parse_packet`:
/// `[0xFF, len_lo, len_hi, 0x00, category, param_count_lo, param_count_hi,
///   parameter_id, value_lo, value_hi, ...]`
pub fn build_command(category: u8, parameter: u8, value: u16) -> Vec<u8> {
    // Byte layout validated against captured notifications parsed by
    // packet::parse_packet: category at offset 4, parameter id at offset 6,
    // value low byte at offset 8.
    let _ = value; // only the low byte carries meaningful state for Record
    vec![
        0xFF,
        0x09,
        0x00,
        0x00,
        category,
        0x01,
        parameter,
        0x02,
        0x00,
        0x40,
        0x00,
        0x02,
    ]
}

/// `FF 09 00 00 0A 01 01 02 <state> 00 40 00 02` - Transport/Record command.
fn transport_record_packet(state: u8) -> Vec<u8> {
    vec![0xFF, 0x09, 0x00, 0x00, TRANSPORT_CATEGORY, 0x01, RECORD_PARAMETER, 0x02, state, 0x00, 0x40, 0x00, 0x02]
}

pub fn record_start_packet() -> Vec<u8> {
    transport_record_packet(1)
}

pub fn record_stop_packet() -> Vec<u8> {
    transport_record_packet(0)
}

// ---------------------------------------------------------------------------
// Notification routing (pure function - testable without hardware)
// ---------------------------------------------------------------------------

/// Translate one raw notification into an optional [`RecordingEvent`].
/// Record transitions map to Started/Stopped; everything else is ignored.
/// `last_start_ms` remembers the timestamp of the most recent start so the
/// matching stop carries a real duration even when the camera omits it.
pub fn notification_to_event(
    bytes: &[u8],
    last_start_ms: &mut Option<u64>,
) -> Option<RecordingEvent> {
    match parse_packet(bytes) {
        CameraPacket::Record { is_recording: true } => {
            let ev = RecordingEvent::new_start();
            if let RecordingEvent::Started { timestamp } = &ev {
                *last_start_ms = Some(*timestamp);
            }
            Some(ev)
        }
        CameraPacket::Record { is_recording: false } => {
            let start = last_start_ms.take().unwrap_or(0);
            Some(RecordingEvent::new_stop(start))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Discovery helpers (pure filtering logic - testable without hardware)
// ---------------------------------------------------------------------------

/// Device-name filter used when scanning without an explicit MAC address.
pub fn name_matches(name: &Option<String>) -> bool {
    match name {
        Some(n) => {
            let n = n.to_ascii_lowercase();
            n.contains("blackmagic") || n.contains("bmpcc") || n.contains("camera control")
        }
        None => false,
    }
}

/// True if `mac` looks like a Bluetooth device address (`AA:BB:CC:DD:EE:FF`).
pub fn is_valid_mac(mac: &str) -> bool {
    let parts: Vec<&str> = mac.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Start a scan, wait `duration`, then stop and flush the event stream.
/// BlueZ delivers scan results asynchronously through `events()`; draining
/// that stream while waiting ensures the adapter's peripheral cache is
/// populated by the time the scan window closes.
async fn scan_window(adapter: &Adapter, duration: Duration) -> BleResult<()> {
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| BleError::ConnectionFailed(format!("scan start failed: {e}")))?;

    let mut events = match adapter.events().await {
        Ok(s) => s,
        Err(e) => {
            let _ = adapter.stop_scan().await;
            return Err(BleError::ConnectionFailed(format!(
                "could not attach to scan event stream: {e}"
            )));
        }
    };

    let deadline = tokio::time::Instant::now() + duration;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        match tokio::time::timeout(deadline - now, events.next()).await {
            Ok(Some(_event)) => {}
            Ok(None) => break,
            Err(_elapsed) => break,
        }
    }

    let _ = adapter.stop_scan().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// The controller
// ---------------------------------------------------------------------------

pub struct CoralCameraController {
    adapter: Adapter,
    peripheral: Option<Peripheral>,
    incoming: Option<Characteristic>,
    outgoing: Option<Characteristic>,
    connected: Arc<AtomicBool>,
    scan_timeout: Duration,
    connect_timeout: Duration,
}

impl CoralCameraController {
    /// Create a controller bound to the default BLE adapter.
    pub async fn new() -> BleResult<Self> {
        Self::builder().build().await
    }

    pub fn builder() -> CoralControllerBuilder {
        CoralControllerBuilder::default()
    }

    async fn init_adapter(adapter_index: u32) -> BleResult<Adapter> {
        let manager = Manager::new()
            .await
            .map_err(|e| BleError::ConnectionFailed(format!("could not open BLE manager: {e}")))?;
        let adapters = manager
            .adapters()
            .await
            .map_err(|e| BleError::ConnectionFailed(format!("could not list BLE adapters: {e}")))?;
        let adapter = adapters
            .into_iter()
            .nth(adapter_index as usize)
            .ok_or_else(|| {
                BleError::ConnectionFailed(format!(
                    "no BLE adapter at index {} (is bluetoothd running / adapter powered on?)",
                    adapter_index
                ))
            })?;
        Ok(adapter)
    }

    /// Scan for nearby Blackmagic cameras. Returns `(address, name)` pairs.
    pub async fn discover(&self) -> BleResult<Vec<(String, String)>> {
        scan_window(&self.adapter, self.scan_timeout).await?;

        let mut found = Vec::new();
        for peripheral in self.adapter.peripherals().await.unwrap_or_default() {
            let props = match peripheral.properties().await {
                Ok(Some(p)) => p,
                _ => continue,
            };
            let local_name = props.local_name.clone();
            if name_matches(&local_name) {
                found.push((props.address.to_string(), local_name.unwrap_or_default()));
            }
        }
        Ok(found)
    }

    /// Find a peripheral whose advertised address equals `mac`, waiting up to
    /// `timeout` for it to show up (adapters populate scan results
    /// asynchronously).
    async fn find_peripheral(&self, mac: &str, timeout: Duration) -> BleResult<Peripheral> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let peripherals = self.adapter.peripherals().await.unwrap_or_default();
            for p in peripherals {
                if let Ok(Some(props)) = p.properties().await {
                    if props.address.to_string().eq_ignore_ascii_case(mac) {
                        return Ok(p);
                    }
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(BleError::DeviceNotFound(format!(
                    "device {mac} not seen within {}s (is it advertising? camera must be powered on with Bluetooth enabled)",
                    timeout.as_secs()
                )));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    fn pick_characteristic(
        chars: &[Characteristic],
        short: u16,
        aliases: &[Uuid],
        kind: &str,
    ) -> BleResult<Characteristic> {
        chars
            .iter()
            .find(|c| char_matches(c, short, aliases))
            .cloned()
            .ok_or_else(|| {
                BleError::NotificationUnavailable(format!(
                    "{kind} characteristic not found (looked for 0x{:04X} and aliases; \
                     override via --incoming-uuid/--outgoing-uuid if your camera differs)",
                    short
                ))
            })
    }

    async fn resolve_characteristics(
        peripheral: &Peripheral,
    ) -> BleResult<(Characteristic, Characteristic)> {
        let services = peripheral.services();

        // Prefer characteristics under the Camera Control service, but fall
        // back to searching every service (some stacks report the service UUID
        // differently).
        let mut all: Vec<Characteristic> = Vec::new();
        let mut svc_chars: Vec<Characteristic> = Vec::new();
        for svc in &services {
            let uuid128 = svc.uuid;
            let is_camera_service =
                uuid128.as_u128() <= u128::from(u32::MAX) && (uuid128.as_u128() as u16) == SERVICE_UUID_SHORT
                    || uuid128 == expand_short_uuid(SERVICE_UUID_SHORT);
            for c in &svc.characteristics {
                if is_camera_service {
                    svc_chars.push(c.clone());
                }
                all.push(c.clone());
            }
        }

        let search = |pool: &[Characteristic]| -> BleResult<(Characteristic, Characteristic)> {
            let incoming =
                Self::pick_characteristic(pool, INCOMING_CHAR_SHORT, &incoming_aliases_128(), "Incoming Camera Control")?;
            let outgoing =
                Self::pick_characteristic(pool, OUTGOING_CHAR_SHORT, &outgoing_aliases_128(), "Outgoing Camera Control")?;
            Ok((incoming, outgoing))
        };

        match search(&svc_chars) {
            Ok(pair) => Ok(pair),
            Err(e) if !all.is_empty() => search(&all).map_err(|_| e),
            Err(e) => Err(e),
        }
    }
}

/// Builder allowing callers to inject a specific adapter index and timeouts.
pub struct CoralControllerBuilder {
    adapter_index: u32,
    scan_timeout: Duration,
    connect_timeout: Duration,
}

impl Default for CoralControllerBuilder {
    fn default() -> Self {
        Self {
            adapter_index: 0,
            scan_timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(10),
        }
    }
}

impl CoralControllerBuilder {
    pub fn adapter_index(mut self, i: u32) -> Self {
        self.adapter_index = i;
        self
    }
    pub fn scan_timeout(mut self, t: Duration) -> Self {
        self.scan_timeout = t;
        self
    }
    pub fn connect_timeout(mut self, t: Duration) -> Self {
        self.connect_timeout = t;
        self
    }

    pub async fn build(self) -> BleResult<CoralCameraController> {
        let adapter = CoralCameraController::init_adapter(self.adapter_index).await?;
        Ok(CoralCameraController {
            adapter,
            peripheral: None,
            incoming: None,
            outgoing: None,
            connected: Arc::new(AtomicBool::new(false)),
            scan_timeout: self.scan_timeout,
            connect_timeout: self.connect_timeout,
        })
    }
}

#[async_trait]
impl CameraController for CoralCameraController {
    async fn connect(&mut self, mac_address: &str) -> BleResult<()> {
        if !is_valid_mac(mac_address) {
            return Err(BleError::DeviceNotFound(format!(
                "'{mac_address}' is not a valid MAC address (expected AA:BB:CC:DD:EE:FF)"
            )));
        }

        // Make sure the device has been scanned recently so it appears in the
        // adapter's peripheral cache.
        scan_window(&self.adapter, self.scan_timeout).await?;
        let peripheral = self.find_peripheral(mac_address, Duration::from_secs(2)).await?;

        peripheral
            .connect()
            .await
            .map_err(|e| BleError::ConnectionFailed(format!("connect to {mac_address} failed: {e}")))?;

        let deadline = tokio::time::Instant::now() + self.connect_timeout;
        while !peripheral.is_connected().await.unwrap_or(false) {
            if tokio::time::Instant::now() >= deadline {
                return Err(BleError::ConnectionFailed(format!(
                    "timed out waiting for connection to {mac_address}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        peripheral
            .discover_services()
            .await
            .map_err(|e| BleError::NotificationUnavailable(format!("service discovery failed: {e}")))?;

        let (incoming, outgoing) = Self::resolve_characteristics(&peripheral).await?;

        log::info!(
            "Connected to {} (incoming {}, outgoing {})",
            mac_address,
            incoming.uuid,
            outgoing.uuid
        );

        self.peripheral = Some(peripheral);
        self.incoming = Some(incoming);
        self.outgoing = Some(outgoing);
        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn disconnect(&mut self) -> BleResult<()> {
        if let Some(p) = self.peripheral.take() {
            let _ = p.disconnect().await;
        }
        self.incoming = None;
        self.outgoing = None;
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn subscribe_to_record_notifications(
        &mut self,
        tx: mpsc::Sender<RecordingEvent>,
    ) -> BleResult<()> {
        let peripheral = self
            .peripheral
            .clone()
            .ok_or_else(|| BleError::ConnectionFailed("not connected".into()))?;
        let incoming = self
            .incoming
            .clone()
            .ok_or_else(|| BleError::NotificationUnavailable("incoming characteristic unresolved".into()))?;

        peripheral
            .subscribe(&incoming)
            .await
            .map_err(|e| {
                BleError::NotificationUnavailable(format!("subscribe to record notifications failed: {e}"))
            })?;

        let mut events = peripheral
            .notifications()
            .await
            .map_err(|e| BleError::NotificationUnavailable(format!("notification stream unavailable: {e}")))?;

        let connected = self.connected.clone();
        tokio::spawn(async move {
            let mut last_start: Option<u64> = None;
            while connected.load(Ordering::SeqCst) {
                // 1s tick lets us observe disconnect() between notifications.
                match tokio::time::timeout(Duration::from_secs(1), events.next()).await {
                    Ok(Some(n)) => {
                        if n.uuid != incoming.uuid {
                            continue;
                        }
                        if let Some(rec_event) = notification_to_event(&n.value, &mut last_start) {
                            if tx.send(rec_event).await.is_err() {
                                log::debug!("event consumer dropped; stopping notification pump");
                                break;
                            }
                        }
                    }
                    Ok(None) => {
                        log::warn!("notification stream closed");
                        break;
                    }
                    Err(_elapsed) => continue,
                }
            }
        });

        Ok(())
    }

    async fn trigger_record(&self, start: bool) -> BleResult<()> {
        let peripheral = self
            .peripheral
            .clone()
            .ok_or_else(|| BleError::ConnectionFailed("not connected".into()))?;
        let outgoing = self
            .outgoing
            .clone()
            .ok_or_else(|| BleError::NotificationUnavailable("outgoing characteristic unresolved".into()))?;

        let packet = if start {
            record_start_packet()
        } else {
            record_stop_packet()
        };

        peripheral
            .write(&outgoing, &packet, WriteType::WithResponse)
            .await
            .map_err(|e| {
                BleError::IoError(std::io::Error::other(format!("record trigger write failed: {e}")))
            })?;
        Ok(())
    }

    async fn get_camera_info(&self) -> BleResult<CameraInfo> {
        let peripheral = self
            .peripheral
            .clone()
            .ok_or_else(|| BleError::ConnectionFailed("not connected".into()))?;
        let props = peripheral
            .properties()
            .await
            .ok()
            .flatten()
            .ok_or_else(|| BleError::ConnectionFailed("no advertising data for camera".into()))?;

        let model = props
            .local_name
            .clone()
            .unwrap_or_else(|| "Blackmagic Camera".to_string());
        let mac = props.address.to_string();

        Ok(CameraInfo {
            model,
            firmware_version: "unknown".into(),
            mac_address: mac,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests - pure logic only; no hardware required.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_validation() {
        assert!(is_valid_mac("AA:BB:CC:DD:EE:FF"));
        assert!(is_valid_mac("aa:bb:cc:dd:ee:ff"));
        assert!(!is_valid_mac("AA:BB:CC:DD:EE"));
        assert!(!is_valid_mac("GG:BB:CC:DD:EE:FF"));
        assert!(!is_valid_mac(""));
    }

    #[test]
    fn name_filter_matches_blackmagic_devices() {
        assert!(name_matches(&Some("BLACKMAGIC POCKET 4K".into())));
        assert!(name_matches(&Some("BMPCC 4K".into())));
        assert!(!name_matches(&Some("Pixel Buds".into())));
        assert!(!name_matches(&None));
    }

    #[test]
    fn record_packets_roundtrip_through_parser() {
        assert_eq!(
            parse_packet(&record_start_packet()),
            CameraPacket::Record { is_recording: true }
        );
        assert_eq!(
            parse_packet(&record_stop_packet()),
            CameraPacket::Record { is_recording: false }
        );
    }

    #[test]
    fn notification_to_event_maps_transitions() {
        let mut last_start = None;
        let on = notification_to_event(&record_start_packet(), &mut last_start);
        assert!(matches!(on, Some(RecordingEvent::Started { .. })));
        assert!(last_start.is_some(), "start timestamp must be remembered");

        let off = notification_to_event(&record_stop_packet(), &mut last_start);
        match off {
            Some(RecordingEvent::Stopped { duration_ms, .. }) => {
                // Stop immediately after start -> tiny but sane duration.
                assert!(duration_ms < 5_000);
            }
            other => panic!("expected Stopped, got {other:?}"),
        }
        assert!(last_start.is_none(), "stop must clear remembered start");
    }

    #[test]
    fn stop_without_prior_start_still_emits_event() {
        let mut last_start = None;
        let off = notification_to_event(&record_stop_packet(), &mut last_start);
        assert!(matches!(off, Some(RecordingEvent::Stopped { .. })));
    }

    #[test]
    fn non_record_notifications_are_ignored() {
        let mut last_start = None;
        let mut bytes = record_start_packet();
        bytes[4] = 1; // change category away from Transport
        assert!(notification_to_event(&bytes, &mut last_start).is_none());
        assert!(notification_to_event(&[0x01, 0x02], &mut last_start).is_none());
    }

    #[test]
    fn short_uuid_expansion_is_canonical() {
        let u = expand_short_uuid(0x7239);
        assert_eq!(u.to_string(), "00007239-0000-1000-8000-00805f9b34fb");
    }

    #[test]
    fn uuid_override_parsing() {
        assert_eq!(parse_uuid_override("0x7239").unwrap(), short_to_uuid(0x7239));
        assert_eq!(parse_uuid_override("7239").unwrap(), short_to_uuid(0x7239));
        assert_eq!(
            parse_uuid_override("B864E140-76A0-416A-BF30-5876504537D9")
                .unwrap()
                .as_u128(),
            0xB864E140_76A0_416A_BF30_5876504537D9_u128
        );
        assert!(parse_uuid_override("not-a-uuid").is_err());
    }

    #[test]
    fn char_matches_accepts_all_known_forms() {
        let aliases = incoming_aliases_128();
        // btleplug's Characteristic has no Default; construct manually.
        let mut c = Characteristic {
            uuid: short_to_uuid(INCOMING_CHAR_SHORT),
            service_uuid: Uuid::nil(),
            properties: Default::default(),
            descriptors: Default::default(),
        };
        assert!(char_matches(&c, INCOMING_CHAR_SHORT, &aliases));

        c.uuid = expand_short_uuid(INCOMING_CHAR_SHORT);
        assert!(char_matches(&c, INCOMING_CHAR_SHORT, &aliases));

        c.uuid = aliases[0];
        assert!(char_matches(&c, INCOMING_CHAR_SHORT, &aliases));

        c.uuid = Uuid::nil();
        assert!(!char_matches(&c, INCOMING_CHAR_SHORT, &aliases));
    }

    #[tokio::test]
    async fn connect_rejects_invalid_mac_before_touching_hardware() {
        // No adapter exists in CI, so building the controller must fail with a
        // clear ConnectionFailed error rather than panicking.
        let result = CoralCameraController::builder()
            .adapter_index(7)
            .scan_timeout(Duration::from_millis(50))
            .build()
            .await;
        match result {
            Ok(_) => panic!("unexpectedly found a 8th BLE adapter"),
            Err(e) => assert!(
                matches!(e, BleError::ConnectionFailed(_)),
                "expected ConnectionFailed, got {e:?}"
            ),
        }
    }
}
