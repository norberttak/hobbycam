#!/usr/bin/env bash
# Mac App Store build: sandboxed app, signed with Apple Distribution, packed into dist/HobbyCAM-<V>-appstore.pkg.
# It never uploads (use Transporter, see RELEASING.md).
# Env: APPSTORE_TEAM_ID, APPSTORE_APP_IDENTITY, APPSTORE_INSTALLER_IDENTITY, APPSTORE_PROFILE. Arg: BUILD (optional)
set -euo pipefail
cd "$(dirname "$0")/.."

MISSING=()
for n in APPSTORE_TEAM_ID APPSTORE_APP_IDENTITY APPSTORE_INSTALLER_IDENTITY APPSTORE_PROFILE; do
  [[ -n "${!n:-}" ]] || MISSING+=("$n")
done
if [[ ${#MISSING[@]} -gt 0 ]]; then
  echo "error: missing ${MISSING[*]}" >&2
  exit 1
fi
[[ -f "$APPSTORE_PROFILE" ]] || { echo "error: profile $APPSTORE_PROFILE not found" >&2; exit 1; }

V=$(cargo pkgid -p hobbycam)
V=${V##*[#@]}
scripts/macos-build.sh --appstore "${1:-$V}"

APP=target/macos/HobbyCAM.app
cp "$APPSTORE_PROFILE" "$APP/Contents/embedded.provisionprofile"
ENT=target/macos/appstore.entitlements
sed "s/@TEAM_ID@/$APPSTORE_TEAM_ID/g" packaging/macos/appstore.entitlements > "$ENT"
plutil -lint "$ENT"
codesign --force --timestamp --options runtime --entitlements "$ENT" --sign "$APPSTORE_APP_IDENTITY" "$APP"
codesign -d --entitlements - "$APP" 2>/dev/null | grep -q "com.apple.security.app-sandbox" \
  || { echo "error: signed app has no app-sandbox entitlement" >&2; exit 1; }

mkdir -p dist
PKG=dist/HobbyCAM-$V-appstore.pkg
productbuild --component "$APP" /Applications --sign "$APPSTORE_INSTALLER_IDENTITY" "$PKG"
pkgutil --check-signature "$PKG"
echo "wrote $PKG (upload with Transporter)"
