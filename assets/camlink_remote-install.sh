#!/bin/bash
set -e

TMPDIR="$(mktemp -d /tmp/camlink.XXXXXX)"
cleanup() { rm -rf "$TMPDIR"; }
trap cleanup EXIT

echo "[Remote Installer] Creating temp workspace..."
cd "$TMPDIR"

echo "[Remote Installer] Downloading repo..."
curl -fsSL \
  -o repo.zip \
  https://github.com/MetalMan1245/blackmagic_camera_bridge/archive/refs/heads/main.zip

echo "[Remote Installer] Extracting..."
unzip -q repo.zip
cd blackmagic_camera_bridge-main

echo "[Remote Installer] Running installer..."
chmod +x install_uninstall-camlink.sh
./install_uninstall-camlink.sh

echo "[Remote Installer] Done."
