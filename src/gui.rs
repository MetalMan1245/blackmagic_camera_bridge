//! Shared, GUI-agnostic core for the camlink Qt front-end.
//!
//! Everything here is pure Rust + tokio so it can be unit-tested without a
//! display server or Bluetooth hardware (set `CAMLINK_GUI_MOCK=1` to exercise
//! the full scan/connect flow against [`MockCameraController`]).
//!
//! The cxx-qt/QML shell in `src/bin/camlink-gui.rs` is a thin binding layer
//! over these types; all logic lives here so regressions are catchable by
//! `cargo test`.

use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use crate::ble::{
    connect_with_retry, CameraController, CoralCameraController, MockCameraController,
};
use crate::config::Config;
use crate::events::{EventDispatcher, RecordingEvent};
use crate::osc::OscSender;

/// UI-facing snapshot of the bridge state (mirrors what CLI logs at startup).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BridgeStatus {
    pub adapter_ready: bool,
    pub connected: bool,
    pub connecting: bool,
    pub scanning: bool,
    pub camera_address: String,
    pub osc_target: String,
}

impl BridgeStatus {
    pub fn connection_label(&self) -> &'static str {
        if self.connecting {
            return "Connecting…";
        }
        match (self.adapter_ready, self.connected) {
            (false, _) => "BLE adapter unavailable",
            (true, true) => "Connected — recording events flow to REAPER",
            (true, false) => "Idle — adapter ready, no camera",
        }
    }
}

/// Normalise hand-typed MAC variants ("aa-bb-cc-dd-ee-ff", lowercase,
/// stray spaces) into the canonical upper-case colon form.
pub fn normalise_mac(raw: &str) -> String {
    raw.trim()
        .replace('-', ":")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(":")
        .to_uppercase()
}

/// Diagnostic lines identical in spirit to the CLI's log stream. Every push
/// also goes through `log::info!` so a terminal running the GUI shows the same
/// output the CLI would have printed.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    lines: Vec<String>,
}

impl Diagnostics {
    pub fn push(&mut self, line: impl Into<String>) {
        let line = line.into();
        log::info!("{}", line);
        self.lines.push(line);
        if self.lines.len() > 500 {
            self.lines.drain(0..self.lines.len() - 500);
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}

enum WorkerCommand {
    Scan,
    Connect(String),
    Disconnect,
}

/// Owns a background tokio runtime thread that serialises all BLE/OSC work so
/// the GUI thread never blocks. Latest results live in shared state the QML
/// shell polls via a Qt timer.
pub struct GuiCore {
    tx: mpsc::UnboundedSender<WorkerCommand>,
    diagnostics: Arc<Mutex<Diagnostics>>,
    status: Arc<Mutex<BridgeStatus>>,
    devices: Arc<Mutex<Vec<(String, String)>>>,
}

impl GuiCore {
    /// Spawn the background worker. `config` supplies OSC host/port defaults.
    pub fn new(config: Config) -> Self {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<WorkerCommand>();
        let status = Arc::new(Mutex::new(BridgeStatus {
            osc_target: format!("{}:{}", config.osc.target_host, config.osc.target_port),
            ..Default::default()
        }));
        let diagnostics = Arc::new(Mutex::new(Diagnostics::default()));
        let devices = Arc::new(Mutex::new(Vec::<(String, String)>::new()));

        let status_w = status.clone();
        let diagnostics_w = diagnostics.clone();
        let devices_w = devices.clone();

        let status_worker = status_w.clone();
        let diagnostics_worker = diagnostics_w.clone();
        let devices_worker = devices_w.clone();

        std::thread::spawn(move || {
            let status_w = status_worker;
            let diagnostics_w = diagnostics_worker;
            let devices_w = devices_worker;
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            rt.block_on(async move {
                let osc_host = config.osc.target_host.clone();
                let osc_port = config.osc.target_port;
                let mock_mode = std::env::var("CAMLINK_GUI_MOCK").is_ok();

                // Controllers are owned by the worker task so trait calls
                // (&mut self) are legal without unsafe aliasing.
                let mut coral: Option<CoralCameraController> = None;
                let mut mock: Option<MockCameraController> = None;
                let mut session_pump: Option<tokio::task::JoinHandle<()>> = None;

                if mock_mode {
                    diagnostics_w
                        .lock()
                        .await
                        .push("Using mock camera controller (CAMLINK_GUI_MOCK=1)");
                    status_w.lock().await.adapter_ready = true;
                } else {
                    match CoralCameraController::new().await {
                        Ok(c) => {
                            diagnostics_w
                                .lock()
                                .await
                                .push("BLE adapter initialised (bluez)");
                            status_w.lock().await.adapter_ready = true;
                            coral = Some(c);
                        }
                        Err(e) => {
                            diagnostics_w
                                .lock()
                                .await
                                .push(format!("BLE adapter init failed: {e}"));
                            diagnostics_w.lock().await.push(
                                "Hint: check bluetoothd is running (bluetoothctl show), \
                                 or set CAMLINK_GUI_MOCK=1 to test without hardware.",
                            );
                        }
                    }
                }

                while let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        WorkerCommand::Scan => {
                            status_w.lock().await.scanning = true;
                            diagnostics_w
                                .lock()
                                .await
                                .push("Scanning for Blackmagic cameras (5s)...");
                            let found: Vec<(String, String)> = if mock_mode {
                                vec![(
                                    "AA:BB:CC:DD:EE:FF".to_string(),
                                    "BMPCC 4K (mock)".to_string(),
                                )]
                            } else if let Some(c) = &coral {
                                match c.discover().await {
                                    Ok(d) => d,
                                    Err(e) => {
                                        diagnostics_w
                                            .lock()
                                            .await
                                            .push(format!("Scan failed: {e}"));
                                        Vec::new()
                                    }
                                }
                            } else {
                                diagnostics_w
                                    .lock()
                                    .await
                                    .push("Scan skipped: no BLE adapter available");
                                Vec::new()
                            };
                            if found.is_empty() {
                                diagnostics_w.lock().await.push(
                                    "No Blackmagic cameras found. Is the camera powered on with Bluetooth enabled?",
                                );
                            } else {
                                diagnostics_w
                                    .lock()
                                    .await
                                    .push(format!("Found {} camera(s):", found.len()));
                                for (a, n) in &found {
                                    diagnostics_w.lock().await.push(format!("{a}  {n}"));
                                }
                            }
                            *devices_w.lock().await = found;
                            status_w.lock().await.scanning = false;
                        }
                        WorkerCommand::Connect(raw_mac) => {
                            let mac = normalise_mac(&raw_mac);
                            if !crate::ble::coral_impl::is_valid_mac(&mac) {
                                diagnostics_w.lock().await.push(format!(
                                    "'{raw_mac}' is not a valid MAC address (expected AA:BB:CC:DD:EE:FF)"
                                ));
                                continue;
                            }
                            status_w.lock().await.connecting = true;
                            status_w.lock().await.camera_address = mac.clone();
                            diagnostics_w
                                .lock()
                                .await
                                .push(format!("Connecting to camera {mac}..."));

                            let mut ok = false;
                            if mock_mode {
                                let mut m = mock.take().unwrap_or_else(MockCameraController::new);
                                match m.connect(&mac).await {
                                    Ok(()) => {
                                        let (tx, rx) = mpsc::channel::<RecordingEvent>(100);
                                        session_pump = Some(tokio::spawn(async move {
                                            let mut rx = rx;
                                            while rx.recv().await.is_some() {}
                                        }));
                                        let _ = m.subscribe_to_record_notifications(tx).await;
                                        diagnostics_w
                                            .lock()
                                            .await
                                            .push(format!("Mock camera connected as {mac}"));
                                        mock = Some(m);
                                        ok = true;
                                    }
                                    Err(e) => diagnostics_w
                                        .lock()
                                        .await
                                        .push(format!("Connect failed: {e}")),
                                }
                            } else if let Some(c) = coral.as_mut() {
                                // Same bounded-retry policy the CLI uses.
                                match connect_with_retry(
                                    c,
                                    &mac,
                                    3,
                                    std::time::Duration::from_millis(500),
                                )
                                .await
                                {
                                    Ok(()) => {
                                        match OscSender::connect(&osc_host, osc_port).await {
                                            Ok(osc) => {
                                                let (tx, rx) = mpsc::channel::<RecordingEvent>(100);
                                                let dispatcher = EventDispatcher::new(osc);
                                                tokio::spawn(async move {
                                                    dispatcher.run(rx).await
                                                });
                                                match c.subscribe_to_record_notifications(tx).await {
                                                    Ok(()) => ok = true,
                                                    Err(e) => diagnostics_w.lock().await.push(
                                                        format!("Subscribe failed: {e}"),
                                                    ),
                                                }
                                            }
                                            Err(e) => diagnostics_w.lock().await.push(format!(
                                                "OSC sender failed ({e}); camera connected but events will not reach REAPER"
                                            )),
                                        }
                                    }
                                    Err(e) => diagnostics_w.lock().await.push(format!(
                                        "Could not connect to camera at {mac}: {e}"
                                    )),
                                }
                            } else {
                                diagnostics_w
                                    .lock()
                                    .await
                                    .push("Cannot connect: no BLE adapter available");
                            }

                            {
                                let mut st = status_w.lock().await;
                                st.connecting = false;
                                st.connected = ok;
                            }
                            if ok {
                                diagnostics_w.lock().await.push(format!(
                                    "camlink ready. Recording events will be sent to {}:{}",
                                    osc_host, osc_port
                                ));
                            }
                        }
                        WorkerCommand::Disconnect => {
                            if let Some(pump) = session_pump.take() {
                                pump.abort();
                            }
                            if let Some(m) = mock.as_mut() {
                                let _ = m.disconnect().await;
                            }
                            if let Some(c) = coral.as_mut() {
                                let _ = c.disconnect().await;
                            }
                            status_w.lock().await.connected = false;
                            diagnostics_w
                                .lock()
                                .await
                                .push("Disconnected from camera");
                        }
                    }
                }
            });
        });

        Self {
            tx: cmd_tx,
            diagnostics: diagnostics_w,
            status: status_w,
            devices: devices_w,
        }
    }

    pub fn scan(&self) {
        let _ = self.tx.send(WorkerCommand::Scan);
    }

    pub fn connect(&self, mac: &str) {
        let _ = self.tx.send(WorkerCommand::Connect(mac.to_string()));
    }

    pub fn disconnect(&self) {
        let _ = self.tx.send(WorkerCommand::Disconnect);
    }

    pub async fn status(&self) -> BridgeStatus {
        self.status.lock().await.clone()
    }

    pub async fn diagnostics(&self) -> Vec<String> {
        self.diagnostics.lock().await.lines().to_vec()
    }

    pub async fn devices(&self) -> Vec<(String, String)> {
        self.devices.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_label_reflects_state() {
        let mut s = BridgeStatus::default();
        assert_eq!(s.connection_label(), "BLE adapter unavailable");
        s.adapter_ready = true;
        assert_eq!(s.connection_label(), "Idle — adapter ready, no camera");
        s.connected = true;
        assert_eq!(
            s.connection_label(),
            "Connected — recording events flow to REAPER"
        );
        s.connecting = true;
        assert_eq!(s.connection_label(), "Connecting…");
    }

    #[test]
    fn normalise_mac_accepts_common_variants() {
        assert_eq!(normalise_mac("aa-bb-cc-dd-ee-ff"), "AA:BB:CC:DD:EE:FF");
        assert_eq!(normalise_mac(" aa:bb:cc:dd:ee:ff "), "AA:BB:CC:DD:EE:FF");
        assert_eq!(normalise_mac("AA:BB:CC:DD:EE:FF"), "AA:BB:CC:DD:EE:FF");
    }

    #[test]
    fn diagnostics_trims_to_500_lines() {
        let mut d = Diagnostics::default();
        for i in 0..600 {
            d.push(format!("line {i}"));
        }
        assert_eq!(d.lines().len(), 500);
        assert_eq!(d.lines().first().unwrap(), "line 100");
        assert_eq!(d.lines().last().unwrap(), "line 599");
    }

    fn mock_config() -> Config {
        Config {
            osc: crate::config::OscConfig {
                target_host: "127.0.0.1".into(),
                target_port: 6666,
                bind_port: 0,
            },
            ..Default::default()
        }
    }

    async fn wait_for(pred: impl Fn(&BridgeStatus) -> bool, core: &GuiCore) -> BridgeStatus {
        for _ in 0..200 {
            let s = core.status().await;
            if pred(&s) {
                return s;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        core.status().await
    }

    // Waits until the predicate holds over (status, devices); async so tests
    // can inspect device listings without `await` inside a sync closure.
    async fn wait_until(
        mut pred: impl FnMut(&BridgeStatus, &[(String, String)]) -> bool,
        core: &GuiCore,
    ) -> (BridgeStatus, Vec<(String, String)>) {
        for _ in 0..400 {
            let s = core.status().await;
            let d = core.devices().await;
            if pred(&s, &d) {
                return (s, d);
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        (core.status().await, core.devices().await)
    }

    // NOTE: tests share the CAMLINK_GUI_MOCK env var, so they run serially.

    async fn mock_core() -> GuiCore {
        std::env::set_var("CAMLINK_GUI_MOCK", "1");
        let core = GuiCore::new(mock_config());
        // Wait until the worker has booted (adapter_ready flips in mock mode).
        wait_for(|s| s.adapter_ready, &core).await;
        core
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mock_scan_finds_camera_and_logs_cli_style_output() {
        let core = mock_core().await;
        core.scan();
        let (s, devs) = wait_until(|s, d| !s.scanning && !d.is_empty(), &core).await;
        assert!(s.adapter_ready);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].1, "BMPCC 4K (mock)");
        let diag = core.diagnostics().await;
        assert!(diag.iter().any(|l| l.contains("Scanning for Blackmagic cameras")));
        assert!(diag.iter().any(|l| l.contains("Found 1 camera(s)")));
        std::env::remove_var("CAMLINK_GUI_MOCK");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mock_connect_sets_connected_and_records_address() {
        let core = mock_core().await;
        core.connect("aa-bb-cc-dd-ee-ff"); // hyphen form should be normalised
        let s = wait_for(|s| s.connected, &core).await;
        assert_eq!(s.camera_address, "AA:BB:CC:DD:EE:FF");
        assert!(s.osc_target.ends_with(":6666"));
        let diag = core.diagnostics().await;
        assert!(diag.iter().any(|l| l.contains("camlink ready")));
        std::env::remove_var("CAMLINK_GUI_MOCK");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn invalid_mac_is_rejected_without_connecting() {
        let core = mock_core().await;
        core.connect("not-a-mac");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let s = core.status().await;
        assert!(!s.connected);
        assert!(!s.connecting);
        let diag = core.diagnostics().await;
        assert!(diag.iter().any(|l| l.contains("not a valid MAC address")));
        std::env::remove_var("CAMLINK_GUI_MOCK");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn disconnect_clears_connected_flag() {
        let core = mock_core().await;
        core.connect("AA:BB:CC:DD:EE:FF");
        wait_for(|s| s.connected, &core).await;
        core.disconnect();
        let s = wait_for(|s| !s.connected, &core).await;
        assert!(!s.connected);
        std::env::remove_var("CAMLINK_GUI_MOCK");
    }
}
