#!/usr/bin/env bash
#
# install_uninstall-camlink.sh — Install camlink to ~/.local with desktop integration.
#
# Idempotent: running multiple times is safe. Detects existing installation
# via manifest and enters uninstallation mode if found.
#
# Manifest location: ~/.local/share/camlink/manifest.txt
# All installed files are tracked here. Only these files will be removed
# during uninstallation.
#
# Usage:
#   ./install_uninstall-camlink.sh           # Install or upgrade
#   ./install_uninstall-camlink.sh --force   # Force reinstall even if manifest exists
#   ./install_uninstall-camlink.sh --uninstall  # Uninstall (alias for detecting manifest)

set -euo pipefail

# --- Configuration ---------------------------------------------------------
PROJECT_NAME="blackmagic-camera-bridge"
INSTALL_ROOT="$HOME/.local/share/camlink"
MANIFEST_FILE="$INSTALL_ROOT/manifest.txt"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ASSETS_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

VENV_DIR="$INSTALL_ROOT/.venv"
RUST_SRC="$(cd -- "$(dirname -- "${ASSETS_DIR}")" && pwd)"

GUI_WRAPPER="$BIN_DIR/camlink-gui"
CLI_BINARY="$BIN_DIR/camlink"
DESKTOP_ENTRY="$DESKTOP_DIR/camlink-gui.desktop"

LOG_FILE="$INSTALL_ROOT/install.log"

# --- Utility Functions -----------------------------------------------------
say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
success() { printf '\033[1;32m✓\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarn:\033[0m %s\n' "$*" >&2; }
error() { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; }
die() { error "$*"; exit 1; }

log() {
    # Create the install root on first use — log() may be called before
    # ensure_directories() (e.g. at the top of main()).
    mkdir -p -- "$(dirname -- "$LOG_FILE")"
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$LOG_FILE" 2>&1
}

# Write a line to the manifest (idempotent)
manifest_add() {
    local path="$1"
    [[ -f "$MANIFEST_FILE" ]] || touch "$MANIFEST_FILE"
    if ! grep -Fxq "$path" "$MANIFEST_FILE" 2>/dev/null; then
        echo "$path" >> "$MANIFEST_FILE"
    fi
    log "Added to manifest: $path"
}

# --- Dependency Detection --------------------------------------------------
detect_package_manager() {
    if command -v pacman >/dev/null 2>&1; then
        echo "pacman"
    elif command -v apt >/dev/null 2>&1; then
        echo "apt"
    elif command -v dnf >/dev/null 2>&1; then
        echo "dnf"
    else
        die "Unknown package manager. Supported: pacman (Arch/CachyOS), apt (Debian/Ubuntu), dnf (Fedora)."
    fi
}

install_system_deps() {
    local pm
    pm=$(detect_package_manager)
    say "Detecting system dependencies (package manager: $pm)"

    # Python
    if ! command -v python3 >/dev/null 2>&1; then
        case $pm in
            pacman) sudo pacman -S --noconfirm python ;;
            apt)    sudo apt install -y python3 ;;
            dnf)    sudo dnf install -y python3 ;;
        esac
    fi

    # Rust toolchain (check for cargo)
    if ! command -v cargo >/dev/null 2>&1; then
        say "Rust not found. Installing via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi

    # D-Bus dev headers (btleplug requirement)
    if [[ "$(uname -s)" == "Linux" ]] && ! pkg-config --exists dbus-1 2>/dev/null; then
        warn "D-Bus development headers not found."
        case $pm in
            pacman) sudo pacman -S --noconfirm dbus pkgconf ;;
            apt)    sudo apt install -y libdbus-1-dev pkg-config ;;
            dnf)    sudo dnf install -y dbus-devel pkgconf-pkg-config ;;
        esac
        say "D-Bus headers installed. You may need to restart your session."
    fi

    # venv module — a separate package on Debian and Ubuntu
    if ! python3 -c 'import venv' 2>/dev/null; then
        warn "The python3 venv module is unavailable."
        case $pm in
            pacman) sudo pacman -S --noconfirm python ;;
            apt)    sudo apt install -y python3-venv ;;
            dnf)    sudo dnf install -y python3 ;;
        esac
    fi

    # NOTE: maturin and PyQt6 are installed into the project virtualenv by
    # install_python_deps(), never system-wide. PEP 668 marks the system
    # interpreter as externally managed, so `pip install --user` is refused
    # on Arch, Debian, and Fedora alike.

    success "System dependencies checked."
}

# --- Installation Logic ----------------------------------------------------
ensure_directories() {
    mkdir -p "$INSTALL_ROOT"
    mkdir -p "$BIN_DIR"
    mkdir -p "$DESKTOP_DIR"
    mkdir -p "$(dirname -- "$LOG_FILE")"
    log "Directories ensured."
}

copy_assets() {
    say "Copying assets to $INSTALL_ROOT"

    # Copy GUI wrapper
    cp "$ASSETS_DIR/camlink-gui-wrapper" "$INSTALL_ROOT/"
    chmod +x "$INSTALL_ROOT/camlink-gui-wrapper"
    manifest_add "$INSTALL_ROOT/camlink-gui-wrapper"

    # Copy .desktop file
    cp "$ASSETS_DIR/camlink-gui.desktop" "$DESKTOP_ENTRY"
    manifest_add "$DESKTOP_ENTRY"

    success "Assets copied."
}

create_venv() {
    say "Creating virtual environment at $VENV_DIR"

    if [[ -x "$VENV_DIR/bin/python" ]]; then
        success "Reusing existing virtual environment."
        return
    fi

    python3 -m venv "$VENV_DIR" \
      || die "Failed to create virtualenv at $VENV_DIR (Debian/Ubuntu: install python3-venv)."

    success "Virtual environment created."
}

install_python_deps() {
    say "Installing Python dependencies into the virtualenv"

    local pip="$VENV_DIR/bin/pip"

    "$pip" install --quiet --upgrade pip
    "$pip" install --quiet "maturin>=1.7,<2.0"
    "$pip" install --quiet "PyQt6>=6.6"

    success "Python dependencies installed."
}

build_extension() {
    say "Building Rust extension (maturin develop --release)"

    # `maturin develop` requires an active virtualenv. Point it at ours via
    # VIRTUAL_ENV rather than shell activation, and run inside a subshell so
    # the cd does not leak into the rest of the script.
    (
        export VIRTUAL_ENV="$VENV_DIR"
        export PATH="$VENV_DIR/bin:$PATH"
        cd "$RUST_SRC"
        maturin develop --release
    )

    log "Extension built successfully."
    success "Rust extension built."
}

build_cli() {
    say "Building CLI binary (cargo build --release --bin camlink)"

    # `maturin develop` only builds the cdylib (--lib --crate-type cdylib),
    # so the camlink executable must be built separately.
    (
        cd "$RUST_SRC"
        cargo build --release --bin camlink
    )

    success "CLI binary built."
}

install_binary() {
    say "Installing CLI binary to $CLI_BINARY"

    local src_bin="$RUST_SRC/target/release/camlink"

    [[ -f "$src_bin" ]] \
      || die "CLI binary not found at '$src_bin'. Run 'cargo build --release' first."

    cp "$src_bin" "$CLI_BINARY"
    chmod +x "$CLI_BINARY"
    manifest_add "$CLI_BINARY"

    success "CLI binary installed."
}

install_gui_wrapper() {
    say "Installing GUI wrapper to $GUI_WRAPPER"

    cat > "$GUI_WRAPPER" <<EOF
#!/usr/bin/env bash
# Auto-generated wrapper for Camlink GUI
# Regenerate by re-running install.sh

set -euo pipefail

PROJECT_ROOT="$INSTALL_ROOT"
VENV_DIR="$VENV_DIR"

[[ -f "\$VENV_DIR/bin/activate" ]] || {
    echo "ERROR: Virtual environment not found at \$VENV_DIR"
    echo "Run 'camlink-gui' again to recreate installation."
    exit 1
}

source "\$VENV_DIR/bin/activate"

if ! python -c 'import blackmagic_camera_bridge' 2>/dev/null; then
    echo "ERROR: Rust extension not importable."
    echo "Re-run 'install.sh' to rebuild."
    exit 1
fi

exec python "\$PROJECT_ROOT/gui.py" "\$@"
EOF

    chmod +x "$GUI_WRAPPER"
    manifest_add "$GUI_WRAPPER"

    # Also create a copy of gui.py
    cp "$ASSETS_DIR/../src/python/gui.py" "$INSTALL_ROOT/gui.py"
    manifest_add "$INSTALL_ROOT/gui.py"

    success "GUI wrapper installed."
}

write_manifest() {
    # Mark the manifest with installation metadata
    {
        echo "# camlink installation manifest"
        echo "# Generated: $(date -Iseconds)"
        echo "# DO NOT EDIT MANUALLY"
        echo "#"
    } > "$MANIFEST_FILE"

    # Add all tracked files
    manifest_add "$CLI_BINARY"
    manifest_add "$GUI_WRAPPER"
    manifest_add "$DESKTOP_ENTRY"
    manifest_add "$INSTALL_ROOT/gui.py"
    manifest_add "$INSTALL_ROOT/camlink-gui-wrapper"
    manifest_add "$INSTALL_ROOT/build.sh"
    manifest_add "$INSTALL_ROOT/run.sh"

    # Track the venv directory as a whole (pattern)
    echo "$VENV_DIR/*" >> "$MANIFEST_FILE"

    success "Manifest written to $MANIFEST_FILE"
}

# --- Uninstallation Logic --------------------------------------------------
do_uninstall() {
    say "Detected existing installation."

    if [[ "${FORCE_UNINSTALL:-}" != "true" ]]; then
        printf '\n\033[1;33mWARNING:\033[0m This will remove:\n'
        printf '\n  CLI binary:     %s\n' "$CLI_BINARY"
        printf '  GUI wrapper:    %s\n' "$GUI_WRAPPER"
        printf '  Desktop entry:  %s\n' "$DESKTOP_ENTRY"
        printf '  Project files:  %s/\n' "$INSTALL_ROOT"
        printf '\nAll files are listed in the manifest.\n'
        printf '\nContinue with uninstallation? [y/N] '

        read -r confirm
        case "$confirm" in
            [yY]|[yY][eE][sS]) ;;
            *) say "Aborted."; exit 0 ;;
        esac
    else
        warn "Force mode: skipping confirmation."
    fi

    say "Removing tracked files from manifest..."

    while IFS= read -r path; do
        # Skip comments and blank lines
        [[ "$path" =~ ^#.*$ || -z "$path" ]] && continue
        # Skip pattern lines (venv/*)
        [[ "$path" =~ \*$ ]] && continue

        if [[ -e "$path" ]]; then
            rm -f "$path"
            log "Removed: $path"
            printf '\r  Removing: %s' "$path"
        fi
    done < "$MANIFEST_FILE"

    # Remove the venv directory (pattern)
    [[ -d "$VENV_DIR" ]] && {
        rm -rf "$VENV_DIR"
        log "Removed directory: $VENV_DIR"
    }

    # Remove the install root (should be empty now except manifest)
    [[ -d "$INSTALL_ROOT" ]] && {
        # Remove anything that wasn't tracked
        find "$INSTALL_ROOT" -mindepth 1 ! -name "manifest.txt" -delete 2>/dev/null || true
        rmdir "$INSTALL_ROOT" 2>/dev/null || true
        log "Removed directory: $INSTALL_ROOT"
    }

    # Remove manifest itself
    [[ -f "$MANIFEST_FILE" ]] && {
        rm -f "$MANIFEST_FILE"
    }

    success "Uninstallation complete."
    printf '\nDone.\n\n'
}

# --- Main Entry Point ------------------------------------------------------
main() {
    log "=== Install script started ==="

    # Parse args
    case "${1:-}" in
        --uninstall)
            FORCE_UNINSTALL="true"
            do_uninstall
            exit 0
            ;;
        --force)
            # Allow force even with existing manifest (will skip uninstall prompt)
            ;;
    esac

    # Check for existing installation
    if [[ -f "$MANIFEST_FILE" ]]; then
        do_uninstall
        exit 0
    fi

    # Normal install flow
    say "Camlink Installation"
    printf '========================================\n\n'

    ensure_directories
    install_system_deps
    create_venv
    install_python_deps
    build_extension
    build_cli
    copy_assets
    install_binary
    install_gui_wrapper
    write_manifest

    printf '\n\n'
    success "Installation complete!"
}

main "$@"
