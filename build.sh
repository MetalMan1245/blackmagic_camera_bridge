#!/usr/bin/env bash
#
# build.sh — provision the Python environment and build the Rust extension.
#
# Idempotent: safe to re-run. Creates .venv on first run, installs maturin,
# then builds the extension in release mode via `maturin develop`.
#
# Overridable environment variables:
#   VENV_DIR     virtualenv location           (default: .venv)
#   PYTHON_BIN   interpreter used to create it (default: python3)

set -euo pipefail

REPO_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$REPO_ROOT"

VENV_DIR="${VENV_DIR:-.venv}"
PYTHON_BIN="${PYTHON_BIN:-python3}"
MATURIN_REQ="maturin>=1.7,<2.0"

say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarn:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# --- preflight -------------------------------------------------------------

command -v "$PYTHON_BIN" >/dev/null 2>&1 \
  || die "$PYTHON_BIN not found. Install Python 3.9 or newer and re-run."

command -v cargo >/dev/null 2>&1 \
  || die "cargo not found. Install the Rust toolchain (https://rustup.rs) and re-run."

# btleplug links against the system D-Bus client library on Linux, via the
# `dbus` crate. Without the development headers the Rust build fails late and
# cryptically, so check up front.
if [[ "$(uname -s)" == "Linux" ]] && ! pkg-config --exists dbus-1 2>/dev/null; then
  warn "pkg-config could not find dbus-1; btleplug needs the D-Bus dev headers."
  printf '      Arch / CachyOS : sudo pacman -S dbus pkgconf\n'
  printf '      Debian / Ubuntu: sudo apt install libdbus-1-dev pkg-config\n'
  printf '      Fedora         : sudo dnf install dbus-devel pkgconf-pkg-config\n'
fi

# --- virtualenv ------------------------------------------------------------

if [[ ! -d "$VENV_DIR" ]]; then
  say "Creating virtual environment in $VENV_DIR"
  "$PYTHON_BIN" -m venv "$VENV_DIR" \
    || die "venv creation failed (Debian/Ubuntu: install python3-venv)."
fi

# shellcheck disable=SC1091
source "$VENV_DIR/bin/activate"

say "Interpreter: $(python --version 2>&1) at $(command -v python)"

# --- python dependencies ---------------------------------------------------

say "Upgrading pip"
python -m pip install --quiet --upgrade pip

say "Installing $MATURIN_REQ"
python -m pip install --quiet --upgrade "$MATURIN_REQ"

# --- rust extension --------------------------------------------------------

say "Building extension (maturin develop --release)"
# PyQt6 is declared under [project].dependencies in pyproject.toml, so maturin
# installs it as part of this step. No separate pip install is needed.
maturin develop --release

say "Build complete."
printf '    Launch the GUI with: ./run.sh\n'
# printf '    Launch the CLI with: cargo run -- --help\n'
