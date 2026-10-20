Blackmagic Camera Bridge — Project Documentation
Overview

Project name: blackmagic-camera-bridge Binary name: camlink Library crate name: blackmagic_camera_bridge (note: hyphens become underscores in use statements) Purpose: Bridge Blackmagic Pocket Cinema Camera 4K Bluetooth record signals to REAPER via OSC, so audio recording starts and stops in sync with camera recording. Language: Rust, 2021 edition Primary platform: Linux (developed on CachyOS, BlueZ stack) License: GNU — see LICENSE for the authoritative text
Problem Statement

Recording professional audio separately from video requires synchronisation between camera and audio recorder. The conventional approaches each carry a cost:

    Timecode generators — effective, but typically £200–300, which is not viable for volunteer work
    Physical sync cables — reliable, but unwanted in a wireless setup
    Manual clap-sync in post — free, but unreliable and slow

The BMPCC4K exposes Bluetooth LE remote control, and REAPER accepts OSC transport commands. Bridging the two produces automatic sync at zero hardware cost.
Current Status
Priority	Goal	Status
P0	OSC transmission to REAPER	Complete — verified live against REAPER
P0	Camera event → OSC event flow	Complete — verified via mock controller
P0	CLI with config file and overrides	Complete
P0	Graceful shutdown	Complete
P1	Mock camera controller	Complete
P1	Notification packet parser	Complete, unvalidated against hardware
P1	Retry with exponential backoff	Complete — primitive only, not yet wired to a supervisor
P1	Unit test suite	17 tests, all passing
P2	Real BLE controller (coral_impl.rs)	Not started
P2	cxx-Qt GUI	Planned
P3	Validated against physical camera	Blocked on hardware access
Architecture
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
File Structure
blackmagic-camera-bridge/
├── Cargo.toml              # Dependencies and metadata
├── Cargo.lock              # Pinned versions (committed — this is a binary crate)
├── LICENSE
├── README.md
├── DOCUMENTATION.md        # This file
├── src/
│   ├── lib.rs              # Module exports
│   ├── main.rs             # CLI entry point, wiring, shutdown handling
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
└── target/                 # Build artifacts (gitignored)

Notes on structure:

    There is currently no integration test target. cargo test reports only unittests src/lib.rs, unittests src/main.rs, and Doc-tests. Any tests/ directory present is not being compiled as a test target — cargo only picks up tests/*.rs or tests/*/main.rs.
    src/cli/mod.rs and src/osc/sender.rs were present in an earlier revision. Whether they still exist should be confirmed; if present but not declared with mod in their parent, they are dead files that are not compiled.
    src/ble/packet.rs is new and declared as a private submodule, re-exported through ble::mod.

Key Components
1. CameraController trait — src/ble/mod.rs
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

The trait is the seam that makes hardware-free development possible. MockCameraController implements it today; CoralCameraController will implement it later; the rest of the application cannot tell the difference.

BleError variants: ConnectionFailed, PairingRequired, DeviceNotFound, NotificationUnavailable, IoError.
2. connect_with_retry — src/ble/mod.rs
pub async fn connect_with_retry(
    controller: &mut dyn CameraController,
    mac_address: &str,
    max_attempts: u32,
    initial_backoff: Duration,
) -> BleResult<()>

Backoff starts at initial_backoff, doubles per attempt, and caps at 5 seconds. Returns the last error if all attempts fail.

Scope limitation, deliberate: this is a tested primitive, not a supervisor. Nothing currently watches the event channel for mid-session disconnects and restarts it. That logic belongs with the real controller, where it can be tested against real failure modes.
3. parse_packet — src/ble/packet.rs
pub const TRANSPORT_CATEGORY: u8 = 10;
pub const RECORD_PARAMETER: u8 = 1;

pub enum CameraPacket {
    Record { is_recording: bool },
    Other { category: u8, parameter: u8 },
    Malformed,
}

pub fn parse_packet(bytes: &[u8]) -> CameraPacket

Byte offsets used: bytes[4] = category, bytes[6] = parameter, bytes[8] = record state (non-zero means recording). Packets shorter than 9 bytes return Malformed.

⚠️ These offsets are reverse-engineered, not documented. They were derived from captured notification samples produced by the "Magic Button 4k" project:
record ON:  FF 09 00 00 0A 01 01 02 02 00 40 00 02
record OFF: FF 09 00 00 0A 01 01 02 00 00 40 00 02

Note that in these notification samples the state byte is at index 8, whereas in the reverse-engineered command bytes the state byte was reported at index 7. That discrepancy is itself a reason for caution. Before trusting this parser with real hardware, validate it against the "Example Protocol Packets" section (page 105) of the official Blackmagic Camera Control PDF, and capture a live session with btmon to compare.

The Other variant exists so unrecognised packets can be logged rather than discarded — on first contact with a real camera, that log is how byte-offset errors will be discovered.
4. RecordingEvent — src/events/mod.rs
pub enum RecordingEvent {
    Started { timestamp: u64 },
    Stopped { timestamp: u64, duration_ms: u64 },
    Error(String),
}

Constructors: RecordingEvent::new_start() and RecordingEvent::new_stop(start_time). Timestamps are milliseconds since the Unix epoch.

Defined once, in mod.rs. An earlier revision duplicated this type and produced an E0428 error; that history is preserved under Resolved Issues.
5. EventDispatcher — src/events/dispatcher.rs
pub struct EventDispatcher {
    osc: OscSender,
    last_recording: Option<bool>,
}

impl EventDispatcher {
    pub fn new(osc: OscSender) -> Self;
    pub async fn run(mut self, rx: mpsc::Receiver<RecordingEvent>);
    async fn dispatch(&mut self, event: RecordingEvent) -> Result<(), OscError>;
}

Responsibilities:

    Receives an injected mpsc::Receiver — the same channel the controller sends into. (An earlier revision created its own internal channel, which meant events could never arrive. That was the structural flaw in the original design.)
    Debounces on last_recording. A duplicate Started when already recording, or a duplicate Stopped when already stopped, is logged as a warning and suppressed. This matters concretely: if the OSC address is ever bound to a REAPER toggle action, one duplicate event would stop a take mid-recording.
    Measures latency — the delta between event timestamp and dispatch time, logged at info for start and debug for stop. On the mock this reads 0 ms. On real hardware it will indicate whether Bluetooth propagation delay threatens sync.

6. OscSender — src/osc/mod.rs
pub struct OscSender {
    socket: UdpSocket,
    target: String,
}

impl OscSender {
    pub async fn connect(host: &str, port: u16) -> OscResult<Self>;
    pub async fn send_record_start(&self) -> OscResult<()>;
    pub async fn send_record_stop(&self, duration_ms: u64) -> OscResult<()>;
}

The socket is bound to 0.0.0.0:0 and then connected to the target. A connected UDP socket fixes the destination (no per-send address resolution) and enables error reporting.

Messages are encoded with rosc and transmitted as raw UDP datagrams.
OSC Interface
Event	OSC address	Arguments
Recording started	/record	float 1.0
Recording stopped	/stop	none

These are REAPER's default-mode transport addresses, and they have been confirmed working against a live REAPER instance on UDP port 6666 with the control surface in Default mode. No "Learn" configuration was required.
REAPER setup

    Options → Preferences → Control/OSC/web
    Add → OSC (Open Sound Control)
    Mode: Default mode (Reaper)
    Note the Local listen port — commonly 8000, but read the actual value from the dialog
    Pattern config: leave as Default
    Ensure REAPER is running before starting camlink

The Listen… button in the Control Surface Settings dialog traces incoming OSC messages in real time, which is the fastest way to distinguish "camlink isn't sending" from "REAPER isn't acting on it".
A note on the ECONNREFUSED error

Because the socket is connected, UDP error reporting is deferred by one send. If nothing is bound to the target port, the kernel stores the ICMP port-unreachable and returns it on the next send. The visible symptom is therefore an error on /stop when the real failure was /record. The error message has been customised to say so explicitly:
OSC send failed: nothing is listening on 127.0.0.1:6666 - is REAPER's OSC
control surface configured? (note: this error may refer to an earlier send)
Configuration

Path: ~/.config/camlink/config.json (tilde expanded via the dirs crate) Format: JSON
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
Precedence

CLI arguments → config file → built-in defaults.

--osc-host and --osc-port have no clap-level defaults; when omitted, the config value is used, and when the config is absent or partial, the built-in default applies.
Partial and malformed configs

Container-level #[serde(default)] is applied to Config and to each sub-struct. Consequences:

    Omitting an entire section (e.g. no osc: key) yields defaults for that section
    Supplying a partial section (e.g. camera with only mac_address) fills the remaining fields from that struct's Default impl
    A malformed or missing file logs a warning and falls back to full defaults — the application does not exit. This is deliberate: a bad config should degrade, not kill the app mid-shoot.

Config fields declared but not yet consumed

These exist in the schema but nothing currently reads them:
Field	Intended consumer
camera.pairing_timeout_ms	coral_impl.rs
camera.service_uuid	coral_impl.rs
camera.incoming_characteristic_uuid	coral_impl.rs
osc.bind_port	Reserved; the sender currently binds to an ephemeral port
logging.level, logging.file	Not wired — logging is currently controlled by the RUST_LOG environment variable
Blackmagic BLE Protocol
GATT identifiers
Role	UUID
Service	291D567A-6D75-11E6-8B77-86F30CA893D3
Incoming Camera Control (notifications — subscribe here)	B864E140-76A0-416A-BF30-5876504537D9
Outgoing Camera Control (writes — manual trigger)	5DD3465F-1AEE-4299-8493-D2ECA2F8E1BB

These come from the official Blackmagic Camera Control manual. They are stored as config values (with these as defaults) rather than hardcoded constants, so a different camera or firmware revision can be accommodated without recompiling.
Packet format

Notifications arrive on the Incoming Camera Control characteristic in the same little-endian, 32-bit-aligned format as the SDI camera control protocol. Record state arrives as an AssignValue packet on the transport category.

The official reference is the Blackmagic Camera Control PDF (free from blackmagicdesign.com/developer). The Bluetooth section begins around page 109, and the Example Protocol Packets section around page 105 is the correct basis for validating the parser.
Important distinction

The REST API for Blackmagic Cameras PDF is not the Bluetooth protocol. It describes an HTTP interface for a different camera lineup and is irrelevant to this project. Do not wire against it.
Dependencies

Versions below are those observed in the last successful build or specified in the manifest. Exact pins live in Cargo.toml.
Used in code today
Crate	Purpose
tokio (1)	Async runtime — features: rt-multi-thread, macros, time, sync, net, signal
async-trait (0.1)	async fn in traits, required for dyn CameraController
thiserror (1)	Error type derivation
log (0.4)	Logging facade
env_logger (0.11)	Log backend, controlled by RUST_LOG
serde (1) + serde_json (1)	Config serialisation
clap (4, derive)	CLI parsing
rosc (0.11)	OSC encoding (encoder::encode) and decoding (decoder::decode_udp)
dirs (5.0.1)	Home-directory resolution for ~ expansion

Note: rosc has no plain decode function. The correct calls are decode_udp, decode_tcp, and decode_tcp_vec; decode_udp returns a tuple of (remainder, packet).
Declared but not yet used
Crate	Intended use
btleplug (0.11)	Cross-platform BLE, backed by BlueZ on Linux — for coral_impl.rs
futures (0.3)	Consuming btleplug's notification Stream
mockall (0.12)	Not currently used — retry tests use a hand-written FlakyCamera struct instead
tokio-test, tempfile	Present in dev-dependencies; not currently referenced

Confirm the exact contents of Cargo.toml before relying on the "declared" rows.
Removed

    uuid — unused
    bluetooth-le — this crate does not exist on crates.io. It appeared in an early manifest draft and caused a resolution failure.

Development Workflow
# Fast feedback while writing code
cargo check

# Full build
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
Environment
# Verbose logging for a single run
env RUST_LOG=debug cargo run -- --mock

# Or for the session
set -x RUST_LOG debug

RUST_LOG is currently the only way to control log verbosity; the logging section of the config file is not yet wired up.
Watch mode
cargo install cargo-watch
cargo watch -x check
cargo watch -x run
Testing Strategy
Layer 1 — Unit tests (no hardware, no network)

Inline #[cfg(test)] modules. Currently 17 tests, all passing:
Module	Count	Coverage
ble::packet	4	Record-on and record-off samples decode correctly; truncated packets return Malformed; unrelated packets classify as Other
ble	3	Error Display formatting; retry recovers after two transient failures in three attempts; retry gives up at exactly the attempt cap
ble::mock	2	Mock connects successfully; mock emits an event within timeout
config	2	Defaults carry the correct Blackmagic UUIDs; partial JSON loads with defaults filled in
osc	2	Real UDP datagrams leave the process, are received by an ephemeral listener, and decode to the expected address and argument
events::dispatcher	4	Start emits /record; stop emits /stop; duplicate start is suppressed (nothing arrives within 2 s); latency under 1 s

All UDP-based tests bind port 0 (ephemeral), so parallel execution cannot cause port collisions. A failure in these is a real defect, not a race.

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
Packet decode from raw bytes	Unit test against reverse-engineered samples	⚠️ Verified against samples only
BLE adapter can scan	bluetoothctl scan on found a device	✅ Verified (Classic, not LE)
BLE Low Energy specifically	—	🔴 Not confirmed
Encrypted characteristic behaviour	—	🔴 Hardware-gated
Real camera notification format	—	🔴 Hardware-gated
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
Troubleshooting
Module not found
error[E0432]: unresolved import `blackmagic_camera_bridge::ble::...`

Check that src/ble/mod.rs exists, that src/lib.rs declares pub mod ble;, and that submodules are declared and re-exported.
Duplicate definitions
error[E0428]: the name `X` is defined multiple times

Search the whole file for the type, check for copy-pasted blocks, and verify impl blocks are not duplicated.
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

Future Work: cxx-Qt GUI
Why cxx-Qt

It is the only actively maintained Rust/Qt binding that exposes the full Qt API. ritual is unmaintained and described in the community as incomplete; qmetaobject is QML-only and has been largely dormant. cxx-Qt is backed by KDAB and referenced in Rust Foundation material.
CMake is not required

Earlier guidance overstated this. cxx-Qt supports a Cargo-only build path: a build.rs using cxx-qt-build performs C++ generation, Qt resource compilation, and linking. The upstream repository includes a qml-minimal-no-cmake example built exactly this way.

Prerequisites on CachyOS:
sudo pacman -S qt6-base qt6-declarative lld mold

The Cargo-only path does not link with GNU ld.bfd (the default linker on most distributions), so lld or mold is required.
Shape of the eventual GUI

Deliberately small:

    Camera MAC address input, persisted to the JSON config
    Connection status indicator
    Scrollable event log showing start/stop timestamps and latency
    Manual trigger buttons for OSC start/stop, for quick verification without a camera

Note that cxx-Qt is QML-based rather than Qt Widgets. For a status window this is unlikely to be a limitation, but it is a different mental model from PyQt6's widget composition.
Sequencing

Build the GUI after the real BLE controller is validated. A GUI doubles the surface area for diagnosing a BLE fault; if the phone-simulator or camera test passes first, the GUI wraps code that is already trusted.
External Resources
Resource	Purpose
https://www.blackmagicdesign.com/developer	Official developer page — Camera Control PDF, Bluetooth section
https://github.com/coral/blackmagic-camera-control	Rust BLE library (reference implementation)
https://github.com/coral/blackmagic-camera-protocol	PROTOCOL.json — machine-readable command definitions
https://docs.rs/btleplug/latest/btleplug/	BLE API documentation
https://docs.rs/rosc/latest/rosc/	OSC API documentation
https://www.kdab.com/cxx-qt/	cxx-Qt documentation
https://github.com/KDAB/cxx-qt	cxx-Qt source, including the no-CMake example
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

Repository

    GitHub: https://github.com/MetalMan1245/blackmagic_camera_bridge
    Author: Malachi Ayers
    Development status: CLI and core logic complete and verified; hardware integration pending
    Next milestone: coral_impl.rs — btleplug adapter scan, connection, notification subscription, and parse_packet wired into the event channel
