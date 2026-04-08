#!/usr/bin/env bash
# deploy.sh — build and push to the Pi in one step.
#
# Usage:
#   ./deploy/deploy.sh <pi-host>
#
# Example:
#   ./deploy/deploy.sh pi@raspberrypi.local
#
# Requires:
#   • cross (cargo install cross) for cross-compilation, OR
#   • an ARM toolchain at arm-linux-gnueabihf-
#   • rsync + ssh access to the Pi

set -euo pipefail

PI="${1:?Usage: $0 <user@host>}"

BINARY_lightd="target/armv7-unknown-linux-gnueabihf/release/lightd"
BINARY_cli="target/armv7-unknown-linux-gnueabihf/release/beatmap-cli"

echo "==> Building for armv7-unknown-linux-gnueabihf"
cross build --release --target armv7-unknown-linux-gnueabihf

echo "==> Uploading binaries to $PI"
rsync -avz \
  "$BINARY_lightd" \
  "$BINARY_cli" \
  "$PI:/usr/local/bin/"

echo "==> Uploading config"
rsync -avz \
  config/lightd.toml \
  config/plans/ \
  "$PI:/etc/luminode-sync/"

echo "==> Uploading service file"
rsync -avz \
  deploy/leds-sync.service \
  "$PI:/etc/systemd/system/"

echo "==> Restarting service"
ssh "$PI" "sudo systemctl daemon-reload && sudo systemctl restart leds-sync.service"

echo "==> Done. Tailing logs:"
ssh "$PI" "journalctl -u leds-sync.service -f --no-pager"
