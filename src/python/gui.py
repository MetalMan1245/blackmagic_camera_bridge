#!/usr/bin/env python3
"""
Camlink PyQt6 GUI — front-end for the Rust CamlinkCore extension.

Usage:
    cd ~/Dev/blackmagic-camera-bridge
    maturin develop --release
    python python/gui.py
"""

import sys
from pathlib import Path

from PyQt6.QtCore import QTimer, Qt
from PyQt6.QtWidgets import (
    QApplication,
    QVBoxLayout,
    QHBoxLayout,
    QPushButton,
    QLabel,
    QTextEdit,
    QListWidget,
    QMainWindow,
    QWidget,
    QLineEdit,
)

# Import the Rust extension
try:
    import blackmagic_camera_bridge
except ImportError:
    print("ERROR: Rust extension not found.")
    print("Run 'maturin develop --release' from the repo root first.")
    sys.exit(1)


class CamlinkWindow(QMainWindow):
    """Main window mirroring GuiCore state via periodic polling."""

    def __init__(self):
        super().__init__()
        self._core: blackmagic_camera_bridge.CamlinkCore | None = None
        self._timer: QTimer | None = None
        self._init_ui()
        self._init_core()

    def _init_ui(self):
        """Build the widget tree. No QML, pure PyQt6."""
        self.setWindowTitle("Camlink — Blackmagic Camera Bridge")
        self.setMinimumSize(600, 500)

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)
        layout.setSpacing(12)
        layout.setContentsMargins(16, 16, 16, 16)

        # Top row: adapter & connection status label
        self._status_label = QLabel("Initializing...")
        self._status_label.setStyleSheet("font-weight: bold; padding: 8px;")
        self._status_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self._status_label)

        # Second row: camera address input
        addr_row = QHBoxLayout()
        addr_row.addWidget(QLabel("Camera MAC:"))
        self._address_input = QLineEdit()
        self._address_input.setPlaceholderText("AA:BB:CC:DD:EE:FF or aa-bb-cc-dd-ee-ff")
        addr_row.addWidget(self._address_input)
        layout.addLayout(addr_row)

        # Third row: action buttons
        btn_row = QHBoxLayout()
        self._scan_btn = QPushButton("Scan for Cameras")
        self._connect_btn = QPushButton("Connect")
        self._disconnect_btn = QPushButton("Disconnect")
        btn_row.addWidget(self._scan_btn)
        btn_row.addWidget(self._connect_btn)
        btn_row.addWidget(self._disconnect_btn)
        layout.addLayout(btn_row)

        # Fourth row: device list
        layout.addWidget(QLabel("Discovered cameras:"))
        self._device_list = QListWidget()
        self._device_list.setAlternatingRowColors(True)
        layout.addWidget(self._device_list)

        # Fifth row: diagnostic log
        layout.addWidget(QLabel("Log:"))
        self._log_area = QTextEdit()
        self._log_area.setReadOnly(True)
        self._log_area.setFontFamily("monospace")
        layout.addWidget(self._log_area)

        # Wire button handlers
        self._scan_btn.clicked.connect(self._on_scan)
        self._connect_btn.clicked.connect(self._on_connect)
        self._disconnect_btn.clicked.connect(self._on_disconnect)

    def _init_core(self):
        """Spawn the Rust worker. Falls back to default config path."""
        try:
            self._core = blackmagic_camera_bridge.CamlinkCore()
        except Exception as e:
            self._status_label.setText(f"Failed to initialise bridge: {e}")
            self._log(f"[error] Initialisation failed: {e}")
            return

        # Start polling timer (250 ms → 4 Hz)
        self._timer = QTimer(self)
        self._timer.timeout.connect(self._poll)
        self._timer.start(250)

        self._log("[info] Rust bridge initialised. Ready.")

    def _poll(self):
        """Pull latest snapshots from Rust and update widgets."""
        if not self._core:
            return

        status = self._core.status()
        devices = self._core.devices()
        diagnostics = self._core.diagnostics()

        # Update status label
        self._status_label.setText(status.connection_label)

        # Update device list
        self._device_list.clear()
        for addr, name in devices:
            self._device_list.addItem(f"{addr}  —  {name}")

        # Update log area — diff-based append to avoid flicker
        prev_text = self._log_area.toPlainText()
        lines = "\n".join(diagnostics)
        if lines != prev_text.strip():
            self._log_area.setPlainText(lines)

    def _on_scan(self):
        if not self._core or self._core.status().scanning:
            return
        self._core.scan()
        self._log("[info] Scanning for cameras...")

    def _on_connect(self):
        if not self._core:
            return
        raw = self._address_input.text().strip()
        if not raw:
            self._log("[warn] No camera address entered.")
            return
        self._core.connect(raw)
        self._log(f"[info] Connect requested: {raw}")

    def _on_disconnect(self):
        if not self._core:
            return
        self._core.disconnect()
        self._log("[info] Disconnect requested.")

    def _log(self, msg: str):
        """Append a timestamped message to the log area."""
        import time
        ts = time.strftime("%H:%M:%S")
        current = self._log_area.toPlainText()
        new_entry = f"[{ts}] {msg}\n"
        self._log_area.setPlainText(current.rstrip() + "\n" + new_entry)


def main():
    app = QApplication(sys.argv)
    win = CamlinkWindow()
    win.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
