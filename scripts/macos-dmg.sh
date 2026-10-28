#!/usr/bin/env bash
# Signs target/macos/HobbyCAM.app (Developer ID, or ad-hoc without an identity), packs it into dist/*.dmg,
# and notarizes + staples when the notary key is given.
# Env (optional): MACOS_SIGNING_IDENTITY; NOTARY_KEY_PATH, NOTARY_KEY_ID, NOTARY_ISSUER (all three or none, need the identity)
# or NOTARY_PROFILE (a `xcrun notarytool store-credentials` keychain profile, for local use)
set -euo pipefail
cd "$(dirname "$0")/.."

ID=${MACOS_SIGNING_IDENTITY:-}
NOTARY_VARS=(NOTARY_KEY_PATH NOTARY_KEY_ID NOTARY_ISSUER)
SET=() MISSING=()
for n in "${NOTARY_VARS[@]}"; do
  if [[ -n "${!n:-}" ]]; then SET+=("$n"); else MISSING+=("$n"); fi
done
if [[ ${#SET[@]} -gt 0 && ${#MISSING[@]} -gt 0 ]]; then
  echo "error: notarization needs all of ${NOTARY_VARS[*]}; missing: ${MISSING[*]}" >&2
  exit 1
fi
NOTARIZE=0
if [[ -n "${NOTARY_PROFILE:-}" && ${#SET[@]} -gt 0 ]]; then
  echo "error: set either NOTARY_PROFILE or ${NOTARY_VARS[*]}, not both" >&2
  exit 1
fi
if [[ ${#SET[@]} -eq ${#NOTARY_VARS[@]} || -n "${NOTARY_PROFILE:-}" ]]; then
  [[ -n "$ID" ]] || { echo "error: notarization needs MACOS_SIGNING_IDENTITY" >&2; exit 1; }
  NOTARIZE=1
fi

V=$(cargo pkgid -p hobbycam)
V=${V##*[#@]}
APP=target/macos/HobbyCAM.app
[[ -d "$APP" ]] || { echo "error: $APP missing, run scripts/macos-build.sh first" >&2; exit 1; }

if [[ -n "$ID" ]]; then
  codesign --force --options runtime --timestamp --sign "$ID" "$APP"
  DMG=dist/HobbyCAM-$V-macos-arm64.dmg
else
  echo "warning: MACOS_SIGNING_IDENTITY not set, ad-hoc signing (unsigned build)" >&2
  codesign --force --sign - "$APP"
  DMG=dist/HobbyCAM-$V-macos-arm64-unsigned.dmg
fi
scripts/check-macos-bundle.sh "$APP"

STAGE=target/macos/dmg-stage
rm -rf "$STAGE"
mkdir -p "$STAGE" dist
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
cp LICENSE "$STAGE/LICENSE.txt"
hdiutil create -volname "HobbyCAM $V" -srcfolder "$STAGE" -format UDZO -ov "$DMG"

if [[ -n "$ID" ]]; then
  codesign --timestamp --sign "$ID" "$DMG"
fi
if [[ $NOTARIZE -eq 1 ]]; then
  if [[ -n "${NOTARY_PROFILE:-}" ]]; then
    KEY=(--keychain-profile "$NOTARY_PROFILE")
  else
    KEY=(--key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER")
  fi
  OUT=$(xcrun notarytool submit "$DMG" "${KEY[@]}" --wait --output-format json) || true
  echo "$OUT"
  STATUS=$(plutil -extract status raw - <<<"$OUT" 2>/dev/null || echo unknown)
  if [[ "$STATUS" != "Accepted" ]]; then
    SUB=$(plutil -extract id raw - <<<"$OUT" 2>/dev/null || true)
    echo "error: notarization status '$STATUS'" >&2
    if [[ -n "$SUB" ]]; then xcrun notarytool log "$SUB" "${KEY[@]}" || true; fi
    exit 1
  fi
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
  spctl -a -t open --context context:primary-signature -vv "$DMG"
fi
echo "wrote $DMG"
