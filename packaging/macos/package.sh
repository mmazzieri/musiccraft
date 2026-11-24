#!/usr/bin/env bash
# Build, sign and (when the secrets are there) notarize the macOS release file:
#
#   $DIST/musiccraft-<version>-macos-universal.dmg   MusicCraft.app on a drag-to-Applications DMG
#
# Usage: packaging/macos/package.sh [--skip-build]
#
# Signing (env): MACOS_SIGN_IDENTITY (default "-" = ad hoc: Gatekeeper rejects it on other
# Macs), MACOS_KEYCHAIN. Notarization: APPLE_ID, APPLE_PASSWORD (app-specific), APPLE_TEAM_ID.
# Adapted from storytold/craft-launcher (packaging/macos/package.sh, MIT OR Apache-2.0).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/macos"

SKIP_BUILD=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
export MACOSX_DEPLOYMENT_TARGET=11.0   # = LSMinimumSystemVersion
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
SHORT_VERSION="${VERSION%%-*}"
WORK="$CARGO_TARGET_DIR/macos-package"
APP="$WORK/$APP_NAME.app"
DMG="$DIST/$PKG-$VERSION-macos-universal.dmg"

NOTARIZE=0
if [[ "$IDENTITY" = "-" ]]; then
  warn "macOS: ad-hoc signing (no MACOS_SIGN_IDENTITY); the DMG is not notarized"
elif [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  NOTARIZE=1
else
  warn "macOS: APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID incomplete; signed but not notarized"
fi
echo "==> $APP_NAME $VERSION for macOS (universal), identity: $IDENTITY, notarize: $NOTARIZE"

if [[ "$SKIP_BUILD" = 0 ]]; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  (cd "$ROOT" && cargo build --release --locked -p "$PKG" -p "$PKG-cli" "${args[@]}")
fi

rm -rf "$WORK"
mkdir -p "$WORK/bin" "$APP/Contents/MacOS" "$APP/Contents/Resources"
for b in "$PKG" "$PKG-cli"; do
  inputs=()
  for t in "${TARGETS[@]}"; do inputs+=("$CARGO_TARGET_DIR/$t/release/$b"); done
  lipo -create -output "$WORK/bin/$b" "${inputs[@]}"
  lipo -info "$WORK/bin/$b"
done

sign() {
  local ts=(--timestamp) kc=()
  [[ "$IDENTITY" = "-" ]] && ts=(--timestamp=none)
  [[ -n "${MACOS_KEYCHAIN:-}" ]] && kc=(--keychain "$MACOS_KEYCHAIN")
  codesign --force --sign "$IDENTITY" ${kc[@]+"${kc[@]}"} "${ts[@]}" "$@"
}

notarize() {
  local file="$1" out id status
  echo "==> notarizing $(basename "$file")"
  out="$(xcrun notarytool submit "$file" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" --wait --timeout 1h --output-format json)" || true
  echo "$out"
  id="$(printf '%s' "$out" | plutil -extract id raw -o - - 2>/dev/null || true)"
  status="$(printf '%s' "$out" | plutil -extract status raw -o - - 2>/dev/null || true)"
  if [[ "$status" != "Accepted" ]]; then
    [[ -n "$id" ]] && xcrun notarytool log "$id" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" || true
    echo "error: notarization of $(basename "$file") failed (status: ${status:-unknown})" >&2
    exit 1
  fi
}

# The stapler fetches the ticket from Apple and can drop the connection: retry.
staple() {
  local target="$1" try
  for try in 1 2 3 4; do
    xcrun stapler staple "$target" && xcrun stapler validate "$target" && return 0
    sleep $((try * 20))
  done
  echo "error: stapling $(basename "$target") failed 4 times" >&2
  exit 1
}

# ---- icon: .icns from the committed 1024 px drawing ---------------------------------------
ICONSET="$WORK/$APP_NAME.iconset"
mkdir -p "$ICONSET"
SRC="$ROOT/assets/app-icon/musiccraft-1024.png"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s" "$SRC" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$SRC" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/$APP_NAME.icns"

# ---- MusicCraft.app ----------------------------------------------------------------------
cp "$WORK/bin/$PKG" "$APP/Contents/MacOS/$APP_NAME"
cp "$WORK/bin/$PKG-cli" "$APP/Contents/MacOS/$PKG-cli"
for f in LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md THIRD-PARTY-LICENSES.txt; do cp "$ROOT/$f" "$APP/Contents/Resources/"; done
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' >"$APP/Contents/PkgInfo"
xattr -cr "$APP"
# The program first: signing the main executable seals the others in the bundle.
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/$PKG-cli"
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP/Contents/MacOS/$APP_NAME"
sign --options runtime --entitlements "$HERE/entitlements.plist" "$APP"
codesign --verify --strict --deep --verbose=2 "$APP"
if [[ "$NOTARIZE" = 1 ]]; then
  ditto -c -k --keepParent "$APP" "$WORK/notarize.zip"
  notarize "$WORK/notarize.zip"
  staple "$APP"
  spctl --assess --type execute -vvv "$APP"
fi

# ---- DMG -----------------------------------------------------------------------------------
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/$APP_NAME.app"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
# `hdiutil create` attaches a device while it copies, which fails now and then on CI runners.
for attempt in 1 2 3 4 5; do
  hdiutil create -srcfolder "$STAGE" -volname "$APP_NAME" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$DMG" && break
  [[ "$attempt" = 5 ]] && { echo "error: hdiutil create failed 5 times" >&2; exit 1; }
  sleep $((attempt * 10))
done
sign "$DMG"
codesign --verify --strict --verbose=2 "$DMG"
if [[ "$NOTARIZE" = 1 ]]; then
  notarize "$DMG"
  staple "$DMG"
  spctl --assess --type open --context context:primary-signature -vvv "$DMG"
fi
echo "==> done"
ls -lh "$DMG"
