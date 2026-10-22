Blackmagic Camera Bridge — Project Documentation

# Overview

- **Project name:** blackmagic-camera-bridge
- **Binary name:** camlink (CLI)
- **Library crate name:** blackmagic_camera_bridge (hyphens become underscores in `use` statements)
- **Python module name:** blackmagic_camera_bridge — the same crate, built additionally as a `cdylib` and loaded by Python
- **Purpose:** Bridge Blackmagic Pocket Cinema Camera 4K Bluetooth record signals to REAPER via OSC, so audio recording starts and stops in sync with camera recording.
- **Languages:** Rust (2021 edition) for all logic; Python 3.9+ for the desktop GUI
- **Primary platform:** Linux (developed on CachyOS, BlueZ stack)
- **License:** GNU — see LICENSE for the authoritative text

# Problem Statement

Recording professional audio separately from video requires synchronisation between camera and audio recorder. The conventional approaches each carry a cost:

- **Timecode generators** — effective, but typically £200–300, which is not viable for volunteer work
- **Physical sync cables** — reliable, but unwanted in a wireless setup
- **Manual clap-sync in post** — free, but unreliable and slow

The BMPCC4K exposes Bluetooth LE remote control, and REAPER accepts OSC transport commands. Bridging the two produces automatic sync at zero hardware cost.

# Current Status

| Priority | Goal | Status |
|----------|------|--------|
| P0 | OSC transmission to REAPER | Complete — verified live against REAPER |
| P0 | Camera event → OSC event flow | Complete — verified via mock controller |
| P0 | CLI with config file and overrides | Complete |
| P0 | Graceful shutdown | Complete |
| P1 | Mock camera controller | Complete |
| P1 | Notification packet parser | Complete, unvalidated against hardware |
| P1 | Retry with exponential backoff | Complete — primitive only, not yet wired to a supervisor |
| P1 | Unit test suite | 17 tests, all passing |
| P2 | GUI core (`gui.rs`) | Complete — GUI-agnostic, unit-tested |
| P2 | pyo3 bindings (`python_api.rs`) | Complete |
| P2 | PyQt6 front-end (`src/python/gui.py`) | Implemented — build verification pending |
| P2 | Real BLE controller (`coral_impl.rs`) | Not started |
| P3 | Validated against physical camera | Blocked on hardware access |

The cxx-Qt GUI approach was abandoned in favour of pyo3 + PyQt6. See **Design Decision: abandoning cxx-Qt** for the reasoning; that section is retained deliberately, because the failure modes are instructive.

# Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                       camlink (binary)                       │
│                                                              │
│  ┌────────────────┐                                          │
│  │ CameraController │  trait — two implementations:          │
│  │   (trait)        │    • MockCameraController (works now)  │
│  └────────┬─────────┘    • CoralCameraController (pending)   │
│           │                                                  │
│           │  mpsc::Sender<RecordingEvent>                    │
│           ▼                                                  │
│  ┌────────────────┐                                          │
│  │ EventDispatcher │  debounce + latency measurement         │
│  └────────┬─────────┘                                        │
│           │                                                  │
│           ▼                                                  │
│  ┌────────────────┐                                          │
│  │   OscSender     │  connected UDP socket, rosc encoding    │
│  └────────┬─────────┘                                        │
│           │                                                  │
│  ┌────────▼─────────┐                                        │
│  │ Config (JSON)     │◀── CLI arguments override              │
│  └───────────────────┘                                       │
└──────────────────────────────┬───────────────────────────────┘
                               │  UDP :6666
                               ▼
                        ┌─────────────┐
                        │   REAPER    │
                        │  (OSC mode) │
                        └─────────────┘
```

The GUI does not duplicate this pipeline. It sits *alongside* it and drives the same `GuiCore`:

```
┌──────────────────────────────────────────────┐
│  src/python/gui.py            (PyQt6)        │  widgets, QTimer, event loop
└───────────────────┬──────────────────────────┘
                    │  6 plain method calls (bool / str / list)
┌───────────────────▼──────────────────────────┐
│  src/python_api.rs            (pyo3)         │  sync wrappers + one Runtime
└───────────────────┬──────────────────────────┘
                    │  Arc<GuiCore>
┌───────────────────▼──────────────────────────┐
│  src/gui.rs                                  │  worker thread, channels,
│                                              │  clone-and-return snapshots
└──────────────────────────────────────────────┘
```

The FFI boundary is deliberately tiny: three fire-and-forget commands (`scan`, `connect`, `disconnect`) and three snapshot getters (`status`, `diagnostics`, `devices`). No BLE or OSC work crosses it.

# File Structure

```
blackmagic-camera-bridge/
├── Cargo.toml              # Rust manifest — deps, features, targets
├── Cargo.lock              # Pinned versions (committed — this is a binary crate)
├── pyproject.toml          # maturin / Python packaging metadata
├── build.sh                # Provisions .venv and builds the extension
├── run.sh                  # Launches the PyQt6 GUI
├── LICENSE
├── README.md
├── DOCUMENTATION.md        # This file
├── .gitignore
├── .gitattributes
├── src/
│   ├── lib.rs              # Module exports
│   ├── main.rs             # CLI entry point, wiring, shutdown handling
│   ├── gui.rs              # GUI-agnostic core: GuiCore, BridgeStatus, Diagnostics
│   ├── python_api.rs       # pyo3 bindings — compiled only with `--features python`
│   ├── python/
│   │   └── gui.py          # PyQt6 front-end
│   ├── ble/
│   │   ├── mod.rs          # CameraController trait, BleError, connect_with_retry
│   │   ├── packet.rs       # Notification byte parser
│   │   ├── mock.rs         # MockCameraController
│   │   └── coral_impl.rs   # PLANNED — real btleplug implementation
│   ├── events/
│   │   ├── mod.rs          # RecordingEvent enum
│   │   └── dispatcher.rs   # Event → OSC routing, debounce, latency
│   ├── osc/
│   │   └── mod.rs          # OscSender, UDP transmission
│   └── config/
│       └── mod.rs          # JSON config, defaults, load/save
└── target/                 # Build artefacts (gitignored)
```

**Removed during the GUI migration:** `build.rs`, `src/camlink_module.rs`, `src/bin/camlink-gui.rs`. All three existed solely to serve cxx-Qt code generation and are no longer referenced anywhere.

**Notes on structure:**

- There is still no integration test target. `cargo test` reports only `unittests src/lib.rs`, `unittests src/main.rs`, and `Doc-tests`. Any `tests/` directory present is not being compiled as a test target — cargo only picks up `tests/*.rs` or `tests/*/main.rs`.
- `src/cli/mod.rs` and `src/osc/sender.rs` were present in an earlier revision. Whether they still exist should be confirmed; if present but not declared with `mod` in their parent, they are dead files that are not compiled.
- `src/ble/packet.rs` is declared as a private submodule, re-exported through `ble::mod`.
- `src/python/` is not a Cargo module directory — Cargo ignores it entirely. It is grouped under `src/` purely for locality.

# Key Components

## 1. CameraController trait — `src/ble/mod.rs`

```rust
#[async_trait]
pub trait CameraController: Send + Sync {
    async fn connect(&mut self, mac_address: &str) -> BleResult<()>;
    async fn disconnect(&mut self) -> BleResult<()>;
    async fn subscribe_to_record_notifications(
        &mut self,
        tx: mpsc::Sender<RecordingEvent>,
    ) -> BleResult<()>;
    async fn trigger_record(&self, start: bool) -> BleResult<()>;
    async fn get_camera_info(&self) -> BleResult<CameraInfo>;
}
```

The trait is the seam that makes hardware-free development possible. `MockCameraController` implements it today; `CoralCameraController` will implement it later; the rest of the application cannot tell the difference.

`BleError` variants: `ConnectionFailed`, `PairingRequired`, `DeviceNotFound`, `NotificationUnavailable`, `IoError`.

## 2. connect_with_retry — `src/ble/mod.rs`

```rust
pub async fn connect_with_retry(
    controller: &mut dyn CameraController,
    mac_address: &str,
    max_attempts: u32,
    initial_backoff: Duration,
) -> BleResult<()>
```

Backoff starts at `initial_backoff`, doubles per attempt, and caps at 5 seconds. Returns the last error if all attempts fail.

**Scope limitation, deliberate:** this is a tested primitive, not a supervisor. Nothing currently watches the event channel for mid-session disconnects and restarts it. That logic belongs with the real controller, where it can be tested against real failure modes.

## 3. parse_packet — `src/ble/packet.rs`

```rust
pub const TRANSPORT_CATEGORY: u8 = 10;
pub const RECORD_PARAMETER: u8 = 1;

pub enum CameraPacket {
    Record { is_recording: bool },
    Other { category: u8, parameter: u8 },
    Malformed,
}

pub fn parse_packet(bytes: &[u8]) -> CameraPacket
```

Byte offsets used: `bytes[4]` = category, `bytes[6]` = parameter, `bytes[8]` = record state (non-zero means recording). Packets shorter than 9 bytes return `Malformed`.

⚠️ **These offsets are reverse-engineered, not documented.** They were derived from captured notification samples produced by the "Magic Button 4k" project:

```
record ON:  FF 09 00 00 0A 01 01 02 02 00 40 00 02
record OFF: FF 09 00 00 0A 01 01 02 00 00 40 00 02
```

Note that in these notification samples the state byte is at index 8, whereas in the reverse-engineered command bytes the state byte was reported at index 7. That discrepancy is itself a reason for caution. Before trusting this parser with real hardware, validate it against the "Example Protocol Packets" section (page 105) of the official Blackmagic Camera Control PDF, and capture a live session with `btmon` to compare.

The `Other` variant exists so unrecognised packets can be logged rather than discarded — on first contact with a real camera, that log is how byte-offset errors will be discovered.

## 4. RecordingEvent — `src/events/mod.rs`

```rust
pub enum RecordingEvent {
    Started { timestamp: u64 },
    Stopped { timestamp: u64, duration_ms: u64 },
    Error(String),
}
```

Constructors: `RecordingEvent::new_start()` and `RecordingEvent::new_stop(start_time)`. Timestamps are milliseconds since the Unix epoch.

Defined once, in `mod.rs`. An earlier revision duplicated this type and produced an `E0428` error; that history is preserved under Resolved Issues.

## 5. EventDispatcher — `src/events/dispatcher.rs`

```rust
pub struct EventDispatcher {
    osc: OscSender,
    last_recording: Option<bool>,
}

impl EventDispatcher {
    pub fn new(osc: OscSender) -> Self;
    pub async fn run(mut self, rx: mpsc::Receiver<RecordingEvent>);
    async fn dispatch(&mut self, event: RecordingEvent) -> Result<(), OscError>;
}
```

Responsibilities:

- Receives an **injected** `mpsc::Receiver` — the same channel the controller sends into. (An earlier revision created its own internal channel, which meant events could never arrive. That was the structural flaw in the original design.)
- **Debounces** on `last_recording`. A duplicate `Started` when already recording, or a duplicate `Stopped` when already stopped, is logged as a warning and suppressed. This matters concretely: if the OSC address is ever bound to a REAPER toggle action, one duplicate event would stop a take mid-recording.
- **Measures latency** — the delta between event timestamp and dispatch time, logged at `info` for start and `debug` for stop. On the mock this reads 0 ms. On real hardware it will indicate whether Bluetooth propagation delay threatens sync.

## 6. OscSender — `src/osc/mod.rs`

```rust
pub struct OscSender {
    socket: UdpSocket,
    target: String,
}

impl OscSender {
    pub async fn connect(host: &str, port: u16) -> OscResult<Self>;
    pub async fn send_record_start(&self) -> OscResult<()>;
    pub async fn send_record_stop(&self, duration_ms: u64) -> OscResult<()>;
}
```

The socket is bound to `0.0.0.0:0` and then connected to the target. A connected UDP socket fixes the destination (no per-send address resolution) and enables error reporting.

Messages are encoded with `rosc` and transmitted as raw UDP datagrams.

## 7. GuiCore — `src/gui.rs`

Pure Rust and tokio. Contains no Qt, no Python, and no FFI, which is what allows it to be unit-tested without a display server or Bluetooth hardware.

```rust
pub struct GuiCore {
    tx: mpsc::UnboundedSender<WorkerCommand>,
    diagnostics: Arc<Mutex<Diagnostics>>,
    status: Arc<Mutex<BridgeStatus>>,
    devices: Arc<Mutex<Vec<(String, String)>>>,
}

impl GuiCore {
    pub fn new(config: Config) -> Self;
    pub fn scan(&self);
    pub fn connect(&self, mac: &str);
    pub fn disconnect(&self);
    pub async fn status(&self) -> BridgeStatus;
    pub async fn diagnostics(&self) -> Vec<String>;
    pub async fn devices(&self) -> Vec<(String, String)>;
}
```

**Threading model.** `GuiCore::new` spawns a `std::thread` that builds its own multi-threaded tokio runtime and then blocks on an async loop draining `WorkerCommand` values. All BLE and OSC work happens on that thread. The three public getters take a `tokio::sync::Mutex`, clone the value, and release — critical sections measured in microseconds.

**Commands are fire-and-forget.** `scan()`, `connect()`, and `disconnect()` push onto an unbounded channel and return immediately. The caller observes progress by polling the getters.

**Supporting types:**

- `BridgeStatus` — `adapter_ready`, `connected`, `connecting`, `scanning`, `camera_address`, `osc_target`, plus `connection_label()` which computes the human-readable string. Keeping the wording in Rust means the CLI and the GUI can never disagree about it.
- `Diagnostics` — a ring buffer capped at 500 lines. Every push also goes through `log::info!`, so a terminal running the GUI shows the same output the CLI would have printed.
- `normalise_mac()` — accepts hyphen or colon separators, mixed case, and stray whitespace; emits canonical upper-case colon form.

**Mock mode.** Setting `CAMLINK_GUI_MOCK=1` swaps in `MockCameraController`, so the entire scan → connect → disconnect flow can be exercised with no hardware present.

## 8. python_api.rs — `src/python_api.rs`

Compiled only when the `python` feature is enabled, so a plain `cargo build` or `cargo test` never pulls in pyo3.

```rust
#[pyclass(name = "BridgeStatus", from_py_object)]
#[derive(Clone)]
pub struct PyBridgeStatus { /* mirrors BridgeStatus, all fields #[pyo3(get)] */ }

#[pyclass(name = "CamlinkCore")]
pub struct PyCamlinkCore {
    core: Arc<GuiCore>,
    rt: tokio::runtime::Runtime,
}
```

`PyCamlinkCore` holds one `current_thread` tokio runtime for the lifetime of the object. It exists solely to drive the `tokio::sync::Mutex` acquisitions inside `GuiCore`'s getters — those are clone-and-return, so blocking on them costs microseconds.

**Correction to the earlier cxx-Qt draft:** that version constructed a fresh tokio runtime on *every* poll tick. This one builds a single runtime at construction and reuses it.

The `#[pymodule]` function must be named `blackmagic_camera_bridge` to match `[lib].name` in `Cargo.toml`, because that name generates the `PyInit_` symbol Python looks for on import.

# GUI Architecture

Three layers, each replaceable without touching the others:

| Layer | File | Responsibility |
|-------|------|----------------|
| Core | `src/gui.rs` | Worker thread, channels, state snapshots. No GUI knowledge. |
| Binding | `src/python_api.rs` | Type marshalling and runtime management. No UI logic. |
| Front-end | `src/python/gui.py` | Widgets, layout, timers, user interaction. No BLE knowledge. |

**Data flow is one-directional.** Python calls `scan()` / `connect()` / `disconnect()`; Rust pushes the command onto a channel and returns. A `QTimer` fires every 250 ms, calling `status()`, `devices()`, and `diagnostics()`, and repaints the affected widgets. There are no callbacks from Rust into Python, which removes an entire class of lifetime and re-entrancy bugs.

**Why polling rather than signals.** The operations being observed are coarse-grained: a scan takes ~5 seconds, a connect either succeeds or fails. At 4 Hz the polling cost is negligible, and the resulting code has no shared mutable state across the FFI boundary.

# OSC Interface

| Event | OSC address | Arguments |
|-------|-------------|-----------|
| Recording started | `/record` | float `1.0` |
| Recording stopped | `/stop` | none |

These are REAPER's default-mode transport addresses, and they have been confirmed working against a live REAPER instance on UDP port 6666 with the control surface in Default mode. No "Learn" configuration was required.

## REAPER setup

1. Options → Preferences → Control/OSC/web
2. Add → OSC (Open Sound Control)
3. Mode: **Default mode (Reaper)**
4. Note the **Local listen port** — commonly 8000, but read the actual value from the dialog
5. Pattern config: leave as Default
6. Ensure REAPER is running before starting camlink

The **Listen…** button in the Control Surface Settings dialog traces incoming OSC messages in real time, which is the fastest way to distinguish "camlink isn't sending" from "REAPER isn't acting on it".

## A note on the ECONNREFUSED error

Because the socket is connected, UDP error reporting is deferred by one send. If nothing is bound to the target port, the kernel stores the ICMP port-unreachable and returns it on the *next* send. The visible symptom is therefore an error on `/stop` when the real failure was `/record`. The error message has been customised to say so explicitly:

```
OSC send failed: nothing is listening on 127.0.0.1:6666 - is REAPER's OSC
control surface configured? (note: this error may refer to an earlier send)
```

# Configuration

**Path:** `~/.config/camlink/config.json` (tilde expanded via the `dirs` crate)
**Format:** JSON

```json
{
  "camera": {
    "mac_address": "",
    "pairing_timeout_ms": 5000,
    "service_uuid": "291D567A-6D75-11E6-8B77-86F30CA893D3",
    "incoming_characteristic_uuid": "B864E140-76A0-416A-BF30-5876504537D9"
  },
  "osc": {
    "target_host": "127.0.0.1",
    "target_port": 6666,
    "bind_port": 0
  },
  "logging": {
    "level": "info",
    "file": "/tmp/camlink.log"
  }
}
```

## Precedence

CLI arguments → config file → built-in defaults.

`--osc-host` and `--osc-port` have no clap-level defaults; when omitted, the config value is used, and when the config is absent or partial, the built-in default applies.

## Partial and malformed configs

Container-level `#[serde(default)]` is applied to `Config` and to each sub-struct. Consequences:

- Omitting an entire section (e.g. no `osc:` key) yields defaults for that section
- Supplying a partial section (e.g. `camera` with only `mac_address`) fills the remaining fields from that struct's `Default` impl
- A malformed or missing file logs a warning and falls back to full defaults — the application does not exit. This is deliberate: a bad config should degrade, not kill the app mid-shoot.

The GUI resolves the config path identically to the CLI: `CAMLINK_CONFIG` if set, otherwise `~/.config/camlink/config.json`. `CamlinkCore(config_path=...)` accepts an explicit override.

## Config fields declared but not yet consumed

| Field | Intended consumer |
|-------|-------------------|
| `camera.pairing_timeout_ms` | `coral_impl.rs` |
| `camera.service_uuid` | `coral_impl.rs` |
| `camera.incoming_characteristic_uuid` | `coral_impl.rs` |
| `osc.bind_port` | Reserved; the sender currently binds to an ephemeral port |
| `logging.level`, `logging.file` | Not wired — logging is controlled by the `RUST_LOG` environment variable |

# Blackmagic BLE Protocol

## GATT identifiers

| Role | UUID |
|------|------|
| Service | `291D567A-6D75-11E6-8B77-86F30CA893D3` |
| Incoming Camera Control (notifications — subscribe here) | `B864E140-76A0-416A-BF30-5876504537D9` |
| Outgoing Camera Control (writes — manual trigger) | `5DD3465F-1AEE-4299-8493-D2ECA2F8E1BB` |

These come from the official Blackmagic Camera Control manual. They are stored as config values (with these as defaults) rather than hardcoded constants, so a different camera or firmware revision can be accommodated without recompiling.

## Packet format

Notifications arrive on the Incoming Camera Control characteristic in the same little-endian, 32-bit-aligned format as the SDI camera control protocol. Record state arrives as an `AssignValue` packet on the transport category.

The official reference is the Blackmagic Camera Control PDF (free from blackmagicdesign.com/developer). The Bluetooth section begins around page 109, and the Example Protocol Packets section around page 105 is the correct basis for validating the parser.

## Important distinction

The **REST API for Blackmagic Cameras** PDF is *not* the Bluetooth protocol. It describes an HTTP interface for a different camera lineup and is irrelevant to this project. Do not wire against it

# Dependencies

Versions below are those observed in the last successful build or specified in the manifest. Exact pins live in `Cargo.toml` and `Cargo.lock`.

## Used in code today

| Crate | Purpose |
|-------|---------|
| tokio (1) | Async runtime — features: `rt-multi-thread`, `macros`, `time`, `sync`, `net`, `signal` |
| async-trait (0.1) | `async fn` in traits, required for `dyn CameraController` |
| thiserror (1) | Error type derivation |
| log (0.4) | Logging facade |
| env_logger (0.11) | Log backend, controlled by `RUST_LOG` |
| serde (1) + serde_json (1) | Config serialisation |
| clap (4, derive) | CLI parsing |
| rosc (0.11) | OSC encoding (`encoder::encode`) and decoding (`decoder::decode_udp`) |
| dirs (5.0.1) | Home-directory resolution for `~` expansion |

**Note:** `rosc` has no plain `decode` function. The correct calls are `decode_udp`, `decode_tcp`, and `decode_tcp_vec`; `decode_udp` returns a tuple of `(remainder, packet)`.

## GUI and packaging

| Dependency | Version | Purpose |
|------------|---------|---------|
| pyo3 | 0.29 | Rust ↔ Python bindings. Optional; gated behind the `python` feature |
| maturin | ≥1.7, <2.0 | Build backend — compiles the `cdylib` and installs it into the venv |
| PyQt6 | ≥6.6 | Python-side GUI toolkit |

**pyo3 version floor is not cosmetic.** Python 3.14 support landed in pyo3 0.25; the 0.23 line rejects a 3.14 interpreter outright with *"the configured Python interpreter version (3.14) is newer than PyO3's maximum supported version (3.13)"*. If the venv interpreter is ever upgraded past the pinned pyo3's ceiling, the build fails at the `pyo3-ffi` build script. Check the pyo3 changelog before bumping Python.

Do **not** work around that error with `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1`. It suppresses the check rather than satisfying it, restricts the crate to the stable-ABI surface, and would need to be exported permanently. Upgrade pyo3 instead.

## Declared but not yet used

| Crate | Intended use |
|-------|--------------|
| btleplug (0.11) | Cross-platform BLE, backed by BlueZ on Linux — for `coral_impl.rs` |
| futures (0.3) | Consuming btleplug's notification `Stream` |
| mockall (0.12) | Not currently used — retry tests use a hand-written `FlakyCamera` struct instead |
| tokio-test, tempfile | Present in dev-dependencies; not currently referenced |

Confirm the exact contents of `Cargo.toml` before relying on the "declared" rows.

## Removed

| Crate | Reason |
|-------|--------|
| uuid | Unused |
| bluetooth-le | This crate does not exist on crates.io. It appeared in an early manifest draft and caused a resolution failure |
| cxx-qt, cxx-qt-lib, cxx-qt-build | Abandoned with the cxx-Qt GUI. See **Design Decision: abandoning cxx-Qt** |

# Development Workflow

```bash
# Fast feedback while writing code
cargo check

# Full build (CLI only — no Qt, no Python)
cargo build

# Run
cargo run -- --help
cargo run -- --mock

# Tests
cargo test
cargo test -- --nocapture

# Quality gates — run all three before committing
cargo fmt
cargo clippy -- -D warnings
cargo test
GUI workflow
# Provision the venv and build the extension (idempotent)
./build.sh

# Launch the GUI
./run.sh

# Launch without Bluetooth hardware
CAMLINK_GUI_MOCK=1 ./run.sh

build.sh and run.sh are bash, not fish, so they remain usable by anyone who clones the repository. They work identically when invoked from a fish shell.

Rebuild discipline. maturin develop must be re-run after any change to Rust code that the Python layer touches — including src/gui.rs, not just src/python_api.rs. Editing only src/python/gui.py requires no rebuild.

Feature gating. pyo3 is an optional dependency behind the python feature. maturin develop enables python and extension-module automatically via [tool.maturin].features in pyproject.toml. A plain cargo build never sees pyo3, which is why the CLI has no Python dependency at all.

The extension-module feature suppresses linking against libpython. That is required for a loadable .so but breaks cargo test, which is precisely why it is not in the default feature set.
Environment
# Verbose logging for a single run
env RUST_LOG=debug cargo run -- --mock

# Or for the session
set -x RUST_LOG debug

RUST_LOG is currently the only way to control log verbosity; the logging section of the config file is not yet wired up.

Rust-side log::info! output goes to stderr and appears in the same terminal as the GUI. env_logger::try_init() is used rather than init() because Python may construct CamlinkCore more than once, and init() panics on a second call.
Watch mode
cargo install cargo-watch
cargo watch -x check
cargo watch -x run

There is no equivalent watch loop for the GUI — a QTimer-driven front-end does not hot-reload. Restart ./run.sh after a rebuild.
Testing Strategy
Layer 1 — Unit tests (no hardware, no network)

Inline #[cfg(test)] modules. Currently 17 tests in the Rust core, all passing:
Module	Count	Coverage
ble::packet	4	Record-on and record-off samples decode correctly; truncated packets return Malformed; unrelated packets classify as Other
ble	3	Error Display formatting; retry recovers after two transient failures in three attempts; retry gives up at exactly the attempt cap
ble::mock	2	Mock connects successfully; mock emits an event within timeout
config	2	Defaults carry the correct Blackmagic UUIDs; partial JSON loads with defaults filled in
osc	2	Real UDP datagrams leave the process, are received by an ephemeral listener, and decode to the expected address and argument
events::dispatcher	4	Start emits /record; stop emits /stop; duplicate start is suppressed (nothing arrives within 2 s); latency under 1 s

Plus five tests in src/gui.rs covering the GUI core:
Test	Coverage
connection_label_reflects_state	All four label branches, including connecting taking precedence
normalise_mac_accepts_common_variants	Hyphen form, colon form, mixed case, surrounding whitespace
diagnostics_trims_to_500_lines	Ring buffer eviction, and that the surviving window is the most recent 500
mock_scan_finds_camera_and_logs_cli_style_output	Full scan flow under CAMLINK_GUI_MOCK=1
mock_connect_sets_connected_and_records_address	Connect flow, MAC normalisation, OSC target formatting

All UDP-based tests bind port 0 (ephemeral), so parallel execution cannot cause port collisions. A failure in these is a real defect, not a race.

Serialisation caveat: the gui.rs tests share the process-global CAMLINK_GUI_MOCK environment variable. They are written to tolerate this, but they are not safe to run concurrently with a future test that asserts the absence of mock mode.

Known gap: src/events/mod.rs has no test module. The RecordingEvent::new_start() timestamp capture and new_stop() duration arithmetic are therefore unverified — the only core logic without test coverage.
Layer 2 — Integration tests (mocks only)

Not yet implemented. There is currently no tests/ target. The natural first integration test is an end-to-end run: mock controller → channel → dispatcher → UDP listener, asserting the full sequence.
Layer 3 — Hardware tests (requires camera)

    Pair the camera via bluetoothctl (or KDE Plasma's Bluetooth settings, which is considerably easier)
    Capture raw notifications with btmon
    Save captures to JSON
    Implement a ReplayCamera that feeds the captured bytes back through parse_packet

Once captured, every subsequent test runs without the camera present. This is the mechanism that makes the hardware dependency a one-time cost.
Verification Status

What has been proven, and how:
Capability	Method	Status
OSC wire format	Unit test — real datagram, decoded and asserted	✅ Verified
OSC delivery to REAPER	Live run, transport engaged on screen	✅ Verified
Event flow, debounce, latency	Unit tests	✅ Verified
Config precedence (CLI over file)	Manual run, --osc-port 12345 overrode config	✅ Verified
Config soft-failure	Manual runs with malformed and missing files	✅ Verified
Retry/backoff primitive	Unit test with flaky fake controller	✅ Verified
Graceful shutdown	Manual run, clean disconnect on Ctrl+C	✅ Verified
GUI core state machine	Unit tests under mock mode	✅ Verified
Python extension builds and imports	maturin develop --release, then import blackmagic_camera_bridge	✅ Verified
GUI launches and renders	./run.sh	✅ Verified
Packet decode from raw bytes	Unit test against reverse-engineered samples	⚠️ Verified against samples only
BLE adapter can scan	bluetoothctl scan on found a device	✅ Verified (Classic, not LE)
BLE Low Energy specifically	—	🔴 Not confirmed
Encrypted characteristic behaviour	—	🔴 Hardware-gated
Real camera notification format	—	🔴 Hardware-gated

Note the distinction between "the GUI launches" and "the GUI drives a real camera". The former is verified. The latter cannot be verified until coral_impl.rs exists and hardware is available.
Hardware Preparation
BlueZ prerequisites
systemctl status bluetooth    # confirm the service is running
btmgmt info                   # look for 'le' in the current settings line

Healthy output resembles:
settings: powered bondable ssp br/edr le secure-conn

If le is absent, enable it:
sudo btmgmt le on

Correction to earlier guidance: there is no plugdev group on Arch-based distributions — that is a Debian convention. BlueZ access is governed by D-Bus and polkit tied to the user session. If bluetoothctl runs and registers an agent without sudo, permissions are already correct.

Adapter capability caveat: a successful Bluetooth Classic connection (headphones, phone pairing as an audio device) does not prove that Low Energy works. LE is a separate radio mode. The BMPCC4K requires LE. Confirm le appears in btmgmt info before relying on the adapter.
Pairing the camera

    Open the camera's Bluetooth menu — pairing mode is only active from the camera side
    The camera displays a six-digit code
    Pair from KDE Plasma's Bluetooth settings (easiest), or:

   bluetoothctl
   scan on
   pair XX:XX:XX:XX:XX:XX
   trust XX:XX:XX:XX:XX:XX

    Enter the six-digit code when prompted
    Record the MAC address in ~/.config/camlink/config.json

Once paired and trusted, reconnection is automatic. The application only ever needs to connect, never to pair — which avoids implementing a BlueZ agent handler.
Known Gaps

Ordered by significance:

    coral_impl.rs does not exist. The non-mock code path exits with an explanatory error rather than attempting a connection. This is intentional: an honest failure is preferable to a fabricated success.
    Packet byte offsets are unvalidated. See the warning under parse_packet.
    No mid-session reconnection. connect_with_retry handles initial connection; nothing supervises the channel afterwards.
    RecordingEvent timestamp and duration arithmetic is untested.
    No integration test target.
    Several config fields are inert — see the table under Configuration.
    logging config section is not wired — verbosity is controlled only by RUST_LOG.
    Possible dead files — src/cli/mod.rs and src/osc/sender.rs may exist without being declared as modules.
    No packaging path for the GUI. build.sh provisions a local venv; there is no wheel published, no .desktop entry, and no AppImage. Distributing the GUI currently means cloning the repo and running two scripts.

Resolved Issues

Retained because the symptoms recur in similar projects.
Duplicate type definitions

Symptom: error[E0428]: the name 'RecordingEvent' is defined multiple times Cause: A copy-paste error placed the enum and its impl block twice in src/events/mod.rs. Resolution: Deleted the second occurrence. Lesson: Never generate code for a file whose current contents are unknown.
Missing module exports

Symptom: error[E0432]: unresolved import 'blackmagic_camera_bridge::ble::MockCameraController' Cause: src/lib.rs lacked pub mod declarations, and ble/mod.rs did not declare or re-export its mock submodule. Resolution: Added pub mod declarations in lib.rs; added mod mock; pub use mock::MockCameraController; in ble/mod.rs.
serde(default) versus impl Default

Symptom: Error("missing field 'pairing_timeout_ms'", line: 4, column: 5) when loading a partial config. Cause: An impl Default block does nothing for deserialisation unless serde is told to use it. Field-level #[serde(default)] on the parent handles a missing key; it does not handle a partial object. Resolution: Added container-level #[serde(default)] to CameraConfig, OscConfig, and LoggingConfig. Both levels are required.
Clippy derivable_impls

Symptom: error: this 'impl' can be derived Cause: Config::default() merely delegated to its three sub-structs' defaults. Resolution: Replaced the manual block with #[derive(Default)]. The sub-structs keep manual impls because their defaults are non-trivial.
Non-existent crate

Symptom: error: no matching package named 'bluetooth-le' found Cause: A crate name was written into the manifest without verification. Resolution: Removed. The actual cross-platform BLE crate is btleplug.
Deferred UDP errors

Symptom: ECONNREFUSED reported on /stop when the real failure was /record. Cause: Standard Linux behaviour for connected UDP sockets — the ICMP port-unreachable from the first send is returned on the second. Resolution: Not a bug. The error message was customised to explain the one-send lag.
pyo3 rejects the Python interpreter

Symptom: error: the configured Python interpreter version (3.14) is newer than PyO3's maximum supported version (3.13), raised from the pyo3-ffi build script. Cause: pyo3 = "0.23" was pinned in the manifest, but the venv interpreter was CPython 3.14. Python 3.14 support arrived in pyo3 0.25. Resolution: Raised the requirement to pyo3 = "0.29". Cargo re-resolved past the 0.23.5 lockfile entry automatically. Lesson: A binding crate's supported-interpreter ceiling is a hard build gate, not a warning. Pin the interpreter version alongside the binding version, or expect this class of failure on every Python upgrade.
Partial move in a From implementation

Symptom: error[E0382]: borrow of partially moved value: 's' at the connection_label field of impl From<CoreStatus> for PyBridgeStatus. Cause: Struct-literal field order is evaluated top to bottom. camera_address: s.camera_address moved the String out of s, and the following line then tried to borrow s to call s.connection_label(). Resolution: Compute the label into a local before constructing the struct:
let connection_label = s.connection_label().to_string();
Self {
    camera_address: s.camera_address,
    connection_label,
    // ...
}

Lesson: In a struct literal that consumes fields by move, any method call on the source must be hoisted above the literal.
Deprecated FromPyObject derive on #[pyclass]

Symptom: warning: use of deprecated associated constant ... HasAutomaticFromPyObject Cause: pyo3 0.29 is moving the automatic FromPyObject implementation for Clone-able #[pyclass] types to an opt-in. Resolution: Added from_py_object to the attribute: #[pyclass(name = "BridgeStatus", from_py_object)]. Behaviour is unchanged; the warning disappears and the crate is forward-compatible with the eventual removal.
Design Decision: abandoning cxx-Qt

This section is retained deliberately. The failure modes are instructive, and the same trap will recur in any project that mixes a Rust core with a C++ toolkit.
What was attempted

A GUI built on cxx-Qt, the KDAB binding that generates C++ glue from a #[cxx_qt::bridge] module. The intent was a QML front-end driven by the existing GuiCore.
Why it failed

The initial commit was titled "First draft GUI: DOES NOT COMPILE" and never progressed past that. Three compounding problems:

1. A malformed manifest. The dependency was declared as:
[target.'cfg(feature = "gui")'.dependencies]
cxx-qt = { version = "0.8", optional = true }

Cargo does not support feature = ... predicates inside target.'cfg(...)' — that namespace is evaluated against the platform (target triple, OS, arch), not crate features. Cargo warned and ignored the predicate. The correct mechanism is the [features] section with dep: syntax.

2. A missing build script. cxx-Qt requires cxx-qt-build in [build-dependencies] plus a build.rs invoking CxxQtBuilder. Neither existed. Without them, no bridge code is generated, so the generated header the source referenced was never produced.

3. Hand-written bridge code that did not match the API. Once the build plumbing was addressed, the bridge module itself proved to be speculative syntax rather than working code: field-level #[cxx_qt_qobject] instead of #[qproperty], #[cxx_qt_base] on a field instead of a struct-level declaration, QML registration attributes in the wrong place, and extern "Rust" method declarations where cxx-Qt expects #[qinvokable] on impl methods. It read plausibly but had never been checked against the crate.

The include!("qml_camlinkmodule.h") line was the original error, and it was wrong on two counts: cxx-Qt wires its own generated headers automatically, and since 0.7 the generated filename derives from the Rust source path anyway.
Why the replacement is structurally better

cxx-Qt solves a specific problem: embedding Rust into an existing C++ application. That is not this project. There is no C++ codebase to integrate with, no existing Qt application to extend. Adopting it meant paying the full cost of a bidirectional C++ bridge — codegen, a build script, a C++ toolchain, lld or mold as the linker — to obtain a GUI that talks to its own backend.

The pyo3 + PyQt6 approach inverts that. The FFI surface is six methods exchanging primitives (bool, str, list). There is no codegen step, no C++ in the repository, and no linker constraint. The GuiCore abstraction — written before either GUI attempt — transferred to the new binding layer unchanged, which is the clearest evidence that it was the right seam to draw.

The generalisable lesson: when a binding layer requires you to learn a second build system to call your own code, check whether the binding's actual purpose matches yours. cxx-Qt is excellent at what it is designed for. This was not that.
Cost of the detour

The Rust GUI was abandoned without ever compiling. Nothing was lost from the core: GuiCore, the CLI, the BLE trait, the OSC layer, and the test suite were all unaffected. The only casualties were three files created solely to serve cxx-Qt — build.rs, src/camlink_module.rs, and src/bin/camlink-gui.rs — all now deleted.
Troubleshooting
Module not found
error[E0432]: unresolved import `blackmagic_camera_bridge::ble::...`

Check that src/ble/mod.rs exists, that src/lib.rs declares pub mod ble;, and that submodules are declared and re-exported.
Duplicate definitions
error[E0428]: the name `X` is defined multiple times

Search the whole file for the type, check for copy-pasted blocks, and verify impl blocks are not duplicated.
Python cannot import the extension
ModuleNotFoundError: No module named 'blackmagic_camera_bridge'

Either maturin develop was never run, or it was run against a different interpreter. Confirm the venv is active (source .venv/bin/activate) and that python -c 'import sys; print(sys.executable)' points inside .venv. Re-run ./build.sh.

If the import succeeds but the module is stale, remember that Rust changes require a rebuild — run.sh checks importability but cannot detect staleness.
Extension links against libpython
ImportError: libpython3.14.so.1.0: cannot open shared object file

The extension-module feature was not active. Verify [tool.maturin].features in pyproject.toml includes both python and extension-module, then re-run ./build.sh.
cargo test fails after building the extension

Expected. extension-module suppresses libpython linkage, which a test binary needs. Run cargo test without the feature — it is not in the default set, so a plain cargo test is already correct. Do not add it to default.
OSC messages not reaching REAPER

    Confirm REAPER is running and its OSC control surface is enabled
    Confirm the port matches: ss -ulnp | grep <port>
    Use the Listen… button in Control Surface Settings to see whether messages arrive
    If messages arrive but nothing happens, the address or argument type is the issue — not the transport

BLE connection failures

    Is the camera powered on and in pairing mode?
    Is it paired and trusted? (bluetoothctl info XX:XX:XX:XX:XX:XX)
    Is the Bluetooth service running? (systemctl status bluetooth)
    Does btmgmt info list le among the current settings?

GUI shows "BLE adapter unavailable" with no camera present

Expected. Set CAMLINK_GUI_MOCK=1 to exercise the full flow without hardware.
dbus-1 not found during the Rust build

btleplug links against the system D-Bus client library via the dbus crate. Install the development headers:
Arch / CachyOS  : sudo pacman -S dbus pkgconf
Debian / Ubuntu : sudo apt install libdbus-1-dev pkg-config
Fedora          : sudo dnf install dbus-devel pkgconf-pkg-config

build.sh checks for this up front and prints the relevant command rather than letting the build fail late.
Future Work
Immediate

    coral_impl.rs — the btleplug implementation: adapter scan, connection, notification subscription, parse_packet wired into the event channel. This is the single largest remaining piece and the gate on everything hardware-related.
    Validate packet offsets against the official Blackmagic Camera Control PDF and a live btmon capture.
    Integration test target — mock controller through to a UDP listener.
    Test RecordingEvent arithmetic — the only core logic currently uncovered.

Medium term

    Mid-session supervision — watch the event channel for disconnects and restart the session, reusing connect_with_retry.
    Wire the logging config section so verbosity is controllable without environment variables.
    Persist the camera MAC from the GUI — the address field currently takes input but does not write it back to the config file.
    Manual OSC trigger buttons in the GUI, for verifying REAPER connectivity without a camera.

Longer term

    GUI packaging — a wheel, a .desktop entry, and ideally an AppImage, so distribution does not require a Rust toolchain on the target machine.
    System tray integration — a status icon matching the KDE Plasma desktop, which is a strong argument for the Qt Widgets approach over QML.
    Windows and macOS — btleplug is cross-platform, so the BLE layer should port. The GUI is already portable. The unknowns are BlueZ-specific assumptions in the retry and pairing logic.

External Resources
Resource	Purpose
https://www.blackmagicdesign.com/developer	Official developer page — Camera Control PDF, Bluetooth section
https://github.com/coral/blackmagic-camera-control	Rust BLE library (reference implementation)
https://github.com/coral/blackmagic-camera-protocol	PROTOCOL.json — machine-readable command definitions
https://docs.rs/btleplug/latest/btleplug/	BLE API documentation
https://docs.rs/rosc/latest/rosc/	OSC API documentation
https://pyo3.rs/	pyo3 user guide — feature flags, build configuration
https://www.maturin.rs/	maturin documentation
https://doc.qt.io/qtforpython-6/	PyQt6 / Qt for Python reference

The cxx-Qt links have been removed, as the project no longer uses it.
Contributing
Adding a module

    Create src/<module>/mod.rs
    Declare it in src/lib.rs with pub mod <module>;
    Declare submodules privately: mod submodule;
    Re-export public items: pub use submodule::PublicItem;

Commit messages

Conventional commits:
feat: add mock camera controller
fix(events): resolve duplicate RecordingEvent definition
docs: update DOCUMENTATION.md
test: add integration test for OSC sender
Testing requirements

    New public function → unit test
    New module → at least one integration test
    Breaking API change → bump major version
    Changes to src/gui.rs → re-run ./build.sh before testing the GUI, since the Python layer will otherwise use a stale extension

Repository

    GitHub: https://github.com/MetalMan1245/blackmagic_camera_bridge
    Author: Malachi Ayers
    Development status: CLI, core logic, and GUI complete and verified against mocks; hardware integration pending
    Next milestone: coral_impl.rs — btleplug adapter scan, connection, notification subscription, and parse_packet wired into the event channel
