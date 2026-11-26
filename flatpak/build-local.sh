#!/usr/bin/env bash
set -euo pipefail

# Build, install, and optionally export a single-file .flatpak bundle of Peppermint locally.
# Prerequisites on Fedora:
#   sudo dnf install flatpak-builder
#   flatpak install flathub org.freedesktop.Platform//26.08 org.freedesktop.Sdk//26.08 org.freedesktop.Sdk.Extension.rust-stable//26.08

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID="io.github.tealeafshark.Peppermint"
MANIFEST="$ROOT_DIR/flatpak/${APP_ID}.local.yml"
VENDOR_DIR="$ROOT_DIR/flatpak/vendor"
VENDOR_CFG="$ROOT_DIR/flatpak/vendor-config.toml"
BUILD_DIR="$ROOT_DIR/flatpak/build-dir"
REPO_DIR="$ROOT_DIR/flatpak/repo"
BUNDLE_PATH="$ROOT_DIR/flatpak/peppermint.flatpak"

if ! command -v flatpak-builder >/dev/null 2>&1; then
    echo "Error: flatpak-builder is not installed."
    echo "Install it on Fedora with: sudo dnf install flatpak-builder"
    exit 1
fi

echo "==> Vendoring Cargo crates for offline Flatpak build..."
rm -rf "$VENDOR_DIR"
(
    cd "$ROOT_DIR"
    cargo vendor --locked "$VENDOR_DIR" > "$VENDOR_CFG"
)
# Rewrite absolute path to relative path inside /run/build/peppermint
sed -i "s|$ROOT_DIR/||g" "$VENDOR_CFG"

echo "==> Building and installing Flatpak ($APP_ID)..."
flatpak-builder \
    --disable-rofiles-fuse \
    --force-clean \
    --user \
    --install \
    --repo="$REPO_DIR" \
    "$BUILD_DIR" \
    "$MANIFEST"

echo "==> Exporting shareable single-file bundle to $BUNDLE_PATH..."
flatpak build-bundle "$REPO_DIR" "$BUNDLE_PATH" "$APP_ID"

echo "==> Cleaning up temporary vendor cache..."
rm -rf "$VENDOR_DIR" "$VENDOR_CFG"

echo ""
echo "Done! Launch Peppermint via Flatpak with:"
echo "  flatpak run $APP_ID"
echo ""
echo "Shareable bundle created at:"
echo "  $BUNDLE_PATH"
