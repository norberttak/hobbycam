#!/usr/bin/env bash
# Builds target/macos/HobbyCAM.app (unsigned = ad-hoc signed, Apple silicon).
# Usage: scripts/macos-build.sh [--appstore] [BUILD]   (BUILD defaults to the version)
# Env: THIRD_PARTY = path of THIRD-PARTY-LICENSES.html (required in CI; unset -> warning, skipped)
set -euo pipefail
cd "$(dirname "$0")/.."

V=$(cargo pkgid -p hobbycam)
V=${V##*[#@]}
FEATURES=()
if [[ "${1:-}" == "--appstore" ]]; then
  FEATURES=(--features appstore)
  shift
fi
BUILD=${1:-$V}

export MACOSX_DEPLOYMENT_TARGET=11.0
cargo build --release -p hobbycam --target aarch64-apple-darwin ${FEATURES[@]+"${FEATURES[@]}"}

APP=target/macos/HobbyCAM.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/aarch64-apple-darwin/release/hobbycam "$APP/Contents/MacOS/hobbycam"
sed -e "s/@VERSION@/$V/g" -e "s/@BUILD@/$BUILD/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
cp app/assets/icon/hobbycam.icns "$APP/Contents/Resources/hobbycam.icns"
cp LICENSE README.md app/assets/PHOSPHOR-LICENSE.txt crates/hc-text/fonts/HERSHEY-NOTICE.txt "$APP/Contents/Resources/"
if [[ -n "${THIRD_PARTY:-}" ]]; then
  [[ -f "$THIRD_PARTY" ]] || { echo "THIRD_PARTY=$THIRD_PARTY does not exist" >&2; exit 1; }
  cp "$THIRD_PARTY" "$APP/Contents/Resources/THIRD-PARTY-LICENSES.html"
else
  echo "warning: THIRD_PARTY not set, THIRD-PARTY-LICENSES.html is not bundled" >&2
fi
# ad-hoc seal (the linker signs only the binary); macos-dmg.sh / macos-appstore.sh re-sign
codesign --force --sign - "$APP"
echo "built $APP ($V, build $BUILD)"
