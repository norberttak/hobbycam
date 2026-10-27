#!/usr/bin/env bash
# Regenerates the app icon assets in app/assets/icon/ (macOS: needs iconutil).
set -euo pipefail
cd "$(dirname "$0")/.."

SET=target/icon/HobbyCAM.iconset
rm -rf "$SET"
HOBBYCAM_ICON_DIR="$PWD/$SET" cargo test -p hobbycam make_icons -- --ignored
mkdir -p app/assets/icon
cp "$SET/hobbycam-1024.png" "$SET/hobbycam-256.png" "$SET/hobbycam.ico" app/assets/icon/
# iconutil only accepts the icon_*.png names
rm "$SET/hobbycam-1024.png" "$SET/hobbycam-256.png" "$SET/hobbycam.ico"
iconutil -c icns "$SET" -o app/assets/icon/hobbycam.icns
echo "icons written to app/assets/icon/"
