#!/usr/bin/env bash
# Verifies a HobbyCAM.app bundle. Usage: scripts/check-macos-bundle.sh <path/to/HobbyCAM.app>
set -euo pipefail
APP=$(cd "$(dirname "${1:?usage: $0 <app>}")" && pwd)/$(basename "$1")
cd "$(dirname "$0")/.."

V=$(cargo pkgid -p hobbycam)
V=${V##*[#@]}
PLIST="$APP/Contents/Info.plist"
BIN="$APP/Contents/MacOS/hobbycam"
FAIL=0
fail() { echo "FAIL: $*" >&2; FAIL=1; }
get() { /usr/libexec/PlistBuddy -c "Print:$1" "$PLIST" 2>/dev/null || true; }

plutil -lint "$PLIST" >/dev/null || fail "Info.plist does not lint"
[[ "$(get CFBundleShortVersionString)" == "$V" ]] || fail "CFBundleShortVersionString '$(get CFBundleShortVersionString)' != $V"
[[ "$(get CFBundleIdentifier)" == "io.github.norberttak.hobbycam" ]] || fail "CFBundleIdentifier '$(get CFBundleIdentifier)'"
[[ -x "$BIN" ]] || fail "$BIN is not executable"
ARCHS=$(lipo -archs "$BIN" 2>/dev/null || true)
[[ "$ARCHS" == "arm64" ]] || fail "architectures '$ARCHS', expected exactly arm64"
[[ -f "$APP/Contents/Resources/hobbycam.icns" ]] || fail "Resources/hobbycam.icns missing"
codesign --verify --strict "$APP" || fail "codesign --verify --strict"
INFO=$(codesign -d --verbose=2 "$APP" 2>&1 || true)
if grep -q '^TeamIdentifier=' <<<"$INFO" && ! grep -q '^TeamIdentifier=not set' <<<"$INFO"; then
  grep -q 'flags=0x10000(runtime)' <<<"$INFO" || fail "signed with a Team ID but no hardened runtime"
fi

if [[ $FAIL -ne 0 ]]; then exit 1; fi
echo "bundle OK: $APP ($V, $ARCHS)"
