#!/usr/bin/env bash
#
# run.sh — launch the PyQt6 front-end against the built Rust extension.
#
# Assumes ./build.sh has been run at least once. Any arguments are forwarded
# to the GUI script.
#
# Useful environment variables:
#   GUI_SCRIPT        path to the Python entry point (default: src/python/gui.py)
#   CAMLINK_CONFIG    path to the JSON config file
#   CAMLINK_GUI_MOCK  set to any value to run without Bluetooth hardware
#   RUST_LOG          Rust-side log verbosity (e.g. RUST_LOG=debug)

set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

VENV_DIR="${VENV_DIR:-.venv}"
GUI_SCRIPT="${GUI_SCRIPT:-src/python/gui.py}"

say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[[ -d "$VENV_DIR" ]] \
  || die "$VENV_DIR not found. Run ./build.sh first."

# shellcheck disable=SC1091
source "$VENV_DIR/bin/activate"

[[ -f "$GUI_SCRIPT" ]] \
  || die "GUI script not found at '$GUI_SCRIPT' (override with GUI_SCRIPT=<path>)."

# Fail here with a clear message rather than letting the GUI print its own.
if ! python -c 'import blackmagic_camera_bridge' 2>/dev/null; then
  die "Rust extension not importable. Run ./build.sh first."
fi

say "Starting camlink GUI"
exec python "$GUI_SCRIPT" "$@"
