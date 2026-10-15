Blackmagic Camera Bridge - Project Documentation
Overview

Project Name: blackmagic-camera-bridge (binary name: camlink)
Purpose: Bridge Blackmagic Pocket Cinema Camera 4K Bluetooth record signals to REAPER DAW via OSC
Language: Rust (2021 Edition)
Platform: Linux (CachyOS with BlueZ)
Problem Statement

Recording professional audio separately from video requires precise synchronization between camera and audio recorder. Traditionally this uses:

    Timecode generators ($200-300) - expensive
    Physical sync cables - inconvenient for wireless setups
    Manual clap-sync in post - unreliable and time-consuming

Solution: Use the BMPCC4K's built-in Bluetooth LE remote control to trigger recording start/stop, send OSC signals to REAPER to automatically start/stop audio recording in sync.
Goals
Priority	Goal	Status
P0	BLE camera event detection	In Progress
P0	OSC signal transmission to REAPER	In Progress
P0	CLI for testing without camera	Complete (stub)
P1	Mock controller for development	Complete
P1	Unit test coverage >80%	In Progress
P2	cxx-Qt GUI interface	Planned
P3	Production-ready with real camera	Pending
Architecture
┌─────────────────────────────────────────────────────────────────┐
│                        camlink (binary)                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐       │
│  │   Camera     │───▶│  Event       │───▶│   OSC        │       │
│  │   Controller │    │  Dispatcher  │    │   Sender     │       │
│  │   (BLE)      │    │              │    │              │       │
│  └──────┬───────┘    └──────────────┘    └──────────────┘       │
│         │                                                      │
│         │                                                      │
│  ┌──────▼───────┐                                              │
│  │   Config     │◀────────────── CLI Arguments                  │
│  │   (JSON)     │                                              │
│  └──────────────┘                                              │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              Testing Infrastructure                     │   │
│  │  • MockCameraController (fake BLE)                      │   │
│  │  • MockOscSender (capture messages)                    │   │
│  │  • ReplayCamera (capture/replay real events)          │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘

                          │
                          ▼
                    ┌──────────┐
                    │  REAPER  │
                    │  (OSC)   │
                    └──────────┘
File Structure
blackmagic-camera-bridge/
├── Cargo.toml              # Dependencies & metadata
├── Cargo.lock              # Pinned versions (committed)
├── LICENSE                 # MIT License
├── README.md               # User-facing docs
├── src/
│   ├── lib.rs              # Module exports (pub mod ble, events, osc)
│   ├── main.rs             # CLI entry point
│   ├── ble/
│   │   ├── mod.rs          # CameraController trait definition
│   │   ├── mock.rs         # Mock implementation for testing
│   │   └── coral_impl.rs   # Planned: real btleplug/coral impl
│   ├── events/
│   │   ├── mod.rs          # RecordingEvent enum + impls
│   │   └── dispatcher.rs   # Routes events to OSC sender
│   ├── osc/
│   │   ├── mod.rs          # OscSender struct
│   │   └── sender.rs       # OSC message construction
│   ├── config/
│   │   └── mod.rs          # JSON config loading/saving
│   └── cli/
│       └── mod.rs          # CLI argument parsing (clap)
├── tests/
│   ├── integration.rs      # End-to-end tests
│   └── common/
│       └── mod.rs          # Shared test helpers
└── target/                 # Build artifacts (.gitignore)
Key Components
1. CameraController Trait (src/ble/mod.rs)

Abstracts BLE communication, enabling swap between mock/real implementations.
pub trait CameraController: Send + Sync {
    async fn connect(&mut self, mac_address: &str) -> BleResult<()>;
    async fn disconnect(&mut self) -> BleResult<()>;
    async fn subscribe_to_record_notifications(&mut self, tx: mpsc::Sender<RecordingEvent>) -> BleResult<()>;
    async fn trigger_record(&self, start: bool) -> BleResult<()>;
    async fn get_camera_info(&self) -> BleResult<CameraInfo>;
}

Why? Testability without hardware, future-proofing for multiple backends (btleplug, coral, CEC).
2. RecordingEvent (src/events/mod.rs)

Domain model representing camera state changes.
pub enum RecordingEvent {
    Started { timestamp: u64 },
    Stopped { timestamp: u64, duration_ms: u64 },
    Error(String),
}

Important: Defined ONCE in mod.rs — avoid duplicate definitions (common error!).
3. EventDispatcher (src/events/dispatcher.rs)

Wires RecordingEvent stream to OSC messages.

Current State: Stub implementation with unused channel — needs refactoring to accept injected rx for testing.
4. OscSender (src/osc/mod.rs)

Transmits OSC commands to REAPER.
pub struct OscSender {
    host: String,
    port: u16,
}

Target Actions:

    /camlink/record/start - Start REAPER recording
    /camlink/record/stop - Stop REAPER recording

Dependencies
Crate	Version	Purpose
tokio	1	Async runtime
btleplug	0.11	Cross-platform BLE (uses BlueZ on Linux)
rosc	0.11	OSC encoding/decoding
clap	4	CLI parsing
thiserror	1	Error type definitions
async-trait	0.1	Async trait support
serde/serde_json	1	JSON config
log/env_logger	0.4/0.11	Logging
mockall	0.12	Mock generation for tests
futures	0.3	Stream utilities

Notable Removal: uuid (unused), bluetooth-le (doesn't exist on crates.io).
Development Workflow
Daily Loop
# Fast check before building
cargo check

# Build and run
cargo build
cargo run -- --help

# Watch mode (auto-rebuild on save)
cargo watch -x 'run'

# Run tests
cargo test -- --nocapture
Environment Setup
# Set logging verbosity
set -x RUST_LOG debug

# Run with mock camera
cargo run -- --mock --osc-host 127.0.0.1 --osc-port 6666

# Test mode
cargo test -- --test-threads 1
Code Quality
# Format code
cargo fmt

# Lint checks
cargo clippy -- -D warnings

# Before commit
cargo test && cargo fmt && cargo clippy
Testing Strategy
Layer 1: Unit Tests (no hardware)

    RecordingEvent creation methods
    OscSender message formatting
    Error type display

Location: Inline #[cfg(test)] modules within source files.
Layer 2: Integration Tests (mocks only)

    End-to-end: MockCamera → Event → OscSender
    Channel communication between controller and dispatcher
    Timeout scenarios

Location: tests/integration.rs
Layer 3: Hardware Tests (when camera available)

    Pair camera via bluetoothctl
    Capture raw BLE notifications with btmon
    Save to JSON for replay testing
    Implement ReplayCamera using captured data

Advantage: After initial capture, all future tests work without camera.
Known Issues & Fixes
Issue 1: Duplicate Module Definitions

Symptom: error[E0428]: the name \RecordingEvent` is defined multiple times`

Cause: Copy-paste error in src/events/mod.rs — RecordingEvent defined twice.

Fix: Delete second occurrence (lines 49-95 in broken version).
Issue 2: Missing Module Exports

Symptom: error[E0432]: unresolved import \blackmagic_camera_bridge::ble::MockCameraController``

Cause: src/lib.rs missing pub mod ble; pub mod events; pub mod osc;

Fix: Add module declarations to lib.rs:
pub mod ble;
pub mod events;
pub mod osc;
pub mod config;
Issue 3: Unused Imports

Warning: unused import: 'info' in dispatcher.rs

Cause: Import added but not used yet (placeholder code).

Fix: Remove unused imports or use #[allow(unused_imports)] during development.
Issue 4: Dependency Conflicts

Observed: thiserror v1 and thiserror v2 both pulled in.

Fix: Pin to v1 in Cargo.toml to align with mockall:
thiserror = "1"  # Not "2"
Protocol References
Blackmagic BLE Protocol

    Reverse-engineered source: https://github.com/coral/blackmagic-camera-protocol
    PROTOCOL.json: Machine-readable command definitions
    Record command (start): {255, 9, 0, 0, 10, 1, 1, 2, 0, 0, 64, 0, 2}
    Record command (stop): {255, 9, 0, 0, 10, 1, 1, 0, 0, 0, 64, 0, 2}

REAPER OSC Actions

Configure in REAPER Preferences → Control/OSC/Web:

    /camlink/record/start → Transport: Record
    /camlink/record/stop → Transport: Stop

BlueZ Pairing Process

    Enter pairing mode on camera (Bluetooth menu)
    Camera displays 6-digit code
    bluetoothctl → pair XX:XX:XX:XX:XX:XX
    Enter code when prompted
    trust XX:XX:XX:XX:XX:XX for auto-reconnect

Future Work: cxx-Qt GUI
Why cxx-Qt (not PyQt6)?
Factor	Reason
Language unity	Pure Rust — no Python interop complexity
Performance	Native bindings, no GIL contention
Type safety	Compile-time QML type checking
Long-term	Better for system integration on Linux
Implementation Plan
Phase 1 (Now): CLI + Core Logic ✓
Phase 2: GUI with cxx-Qt
├── Add cxx-qt dependency
├── Create QmlMain.qml
├── Bridge Rust structs to QML
│   ├── CameraController ↔ QObject
│   ├── EventDispatcher ↔ Signal handlers
│   └── OscSender ↔ Properties
└── System tray integration
cxx-Qt Dependencies
cxx-qt = "0.6"
cxx = "1.0"

[build-dependencies]
cxx-build = "1.0"  # For build.rs
Alternative: Keep PyQt6

Consider if: Rapid GUI development prioritized over language purity. Leverage existing PyQt6 expertise from SC2 launcher/VST Helper projects.

Trade-off: Dual-language codebase, but faster iteration.
External Resources
Resource	Purpose
https://github.com/coral/blackmagic-camera-control	Rust BLE library (reference)
https://github.com/coral/blackmagic-camera-protocol	PROTOCOL.json (command defs)
https://docs.rs/btleplug/latest/btleplug/	BLE API docs
https://docs.rs/rosc/latest/rosc/	OSC API docs
https://www.kdab.com/cxx-qt/	cxx-Qt documentation
https://www.blackmagicdesign.com/developer	Official Blackmagic SDK (macOS/iOS)
Contributing Guidelines
Adding New Modules

    Create src/<module>/mod.rs
    Declare in src/lib.rs: pub mod <module>;
    Add submodules as mod submodule; (private)
    Re-export public items: pub use submodule::PublicItem;

Commit Messages

Follow conventional commits:
feat: add mock camera controller
fix(events): resolve duplicate RecordingEvent definition
docs: update PROJECT_STRUCTURE.md
test: add integration test for OSC sender
Testing Requirements

    New public function → unit test
    New module → at least one integration test
    Breaking API change → bump major version

Troubleshooting Common Errors
"Module not found"
error[E0432]: unresolved import `blackmagic_camera_bridge::ble::...`

Check:

    Does src/ble/mod.rs exist?
    Does src/lib.rs have pub mod ble;?
    Are submodules re-exported if needed?

"Duplicate definitions"
error[E0428]: the name `X` is defined multiple times

Check:

    Search entire file for duplicate type definitions
    Check for copy-pasted code
    Verify impl blocks aren't duplicated

BLE Connection Failures
Error: DeviceNotFound("BMPCC4K")

Troubleshoot:

    Is camera powered on?
    Is it paired via bluetoothctl?
    Is BlueZ service running? systemctl status bluetooth
    Is user in plugdev group? groups $USER

OSC Messages Not Received by REAPER

    Confirm REAPER OSC enabled: Preferences → Control/OSC/Web
    Check firewall: sudo ufw allow 6666/udp
    Verify port matches: ss -ulnp | grep 6666
    Test with standalone OSC listener: nc -ul 6666

Repository Links

    GitHub: https://github.com/MetalMan1245/blackmagic_camera_bridge
    Issue Tracker: (create issues for bugs/features)
    Documentation: (this file + Rust doc comments)

Contact

    Author: Malachi Ayers
    Development Status: Early Alpha (CLI stub complete)
    Next Milestone: First successful mock-to-OSC event flow test

Last Updated: October 15, 2026
Rust Edition: 2021
License: GNU
