#!/usr/bin/env bash
# Build and package MusicCraft for Linux (<arch> is x86_64 or aarch64):
#
#   $DIST/musiccraft-<version>-linux-<arch>.deb       Debian, Ubuntu, Mint, ...
#   $DIST/musiccraft-<version>-linux-<arch>.rpm       Fedora, openSUSE, RHEL, ...
#   $DIST/musiccraft-<version>-linux-<arch>.tar.gz    plain tree (bin/, share/)
#   $DIST/musiccraft-<version>-linux-<arch>.AppImage  only with --formats appimage, see below
#
# The AppImage is off by default: its type2 runtime, the loader at the start of every AppImage,
# links libfuse statically, and libfuse is LGPL-2.1, which the license policy (AGENTS.md) keeps
# out of MusicCraft's releases. The code stays for local use and for a future decision.
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"]
#
# Needs: cargo; nfpm for deb/rpm; appimagetool (downloaded and checked if missing). Build on an
# old distro (CI: Ubuntu 22.04, glibc 2.35) so the binary runs on newer ones.
# Adapted from storytold/craft-launcher (packaging/linux/package.sh, MIT OR Apache-2.0).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"

SKIP_BUILD=0
FORMATS="deb rpm tar"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$(printf %s "$2" | tr ',' ' ')"; shift 2 ;;
    -h | --help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
BASENAME="$PKG-$VERSION-linux-$ARCH"
has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

echo "==> $APP_NAME $VERSION for Linux $ARCH ($FORMATS)"
if [[ "$SKIP_BUILD" = 0 ]]; then
  (cd "$ROOT" && cargo build --release --locked -p "$PKG")
fi
BIN="$CARGO_TARGET_DIR/release"
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- FHS tree shared by every format -------------------------------------------------------
install -Dm755 "$BIN/$PKG" "$STAGE/usr/bin/$PKG"
strip "$STAGE/usr/bin/$PKG" 2>/dev/null || true
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/usr/share/icons/"
install -Dm644 "$ROOT/assets/app-icon/musiccraft.svg" "$STAGE/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
mkdir -p "$STAGE/usr/share/doc/$PKG"
copy_docs "$STAGE/usr/share/doc/$PKG"
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi

# ---- .tar.gz -------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb / .rpm ---------------------------------------------------------------------------
if has deb || has rpm; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export NFPM_ARCH="$DEB_ARCH"
  export GLIBC_FLOOR="${GLIBC_FLOOR:-2.35}"
  sed "s|\${STAGE}|$STAGE|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
fi

# ---- AppImage ------------------------------------------------------------------------------
if has appimage; then
  # Pinned tool and runtime, checked by hash (the same pins as craft-launcher).
  APPIMAGETOOL_VERSION=1.9.1
  APPIMAGE_RUNTIME_VERSION=20251108
  case "$ARCH" in
    x86_64)
      APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
      APPIMAGE_RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d ;;
    aarch64)
      APPIMAGETOOL_SHA256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
      APPIMAGE_RUNTIME_SHA256=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444 ;;
  esac
  APPDIR="$WORK/MusicCraft.AppDir"
  cp -R "$STAGE" "$APPDIR"
  install -Dm755 "$HERE/AppRun" "$APPDIR/AppRun"
  cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
  cp "$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
  ln -s "$APP_ID.png" "$APPDIR/.DirIcon"

  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [[ -z "$TOOL" ]]; then
    TOOL="$CARGO_TARGET_DIR/appimagetool-$APPIMAGETOOL_VERSION-$ARCH.AppImage"
    if [[ ! -x "$TOOL" ]]; then
      curl --proto '=https' -fsSL -o "$TOOL.download" \
        "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$ARCH.AppImage"
      check_sha256 "$APPIMAGETOOL_SHA256" "$TOOL.download"
      chmod 755 "$TOOL.download"
      mv "$TOOL.download" "$TOOL"
    fi
    check_sha256 "$APPIMAGETOOL_SHA256" "$TOOL"
  fi
  RUNTIME="$CARGO_TARGET_DIR/appimage-runtime-$APPIMAGE_RUNTIME_VERSION-$ARCH"
  if [[ ! -f "$RUNTIME" ]]; then
    curl --proto '=https' -fsSL -o "$RUNTIME.download" \
      "https://github.com/AppImage/type2-runtime/releases/download/$APPIMAGE_RUNTIME_VERSION/runtime-$ARCH"
    check_sha256 "$APPIMAGE_RUNTIME_SHA256" "$RUNTIME.download"
    mv "$RUNTIME.download" "$RUNTIME"
  fi
  check_sha256 "$APPIMAGE_RUNTIME_SHA256" "$RUNTIME"
  OUT="$(cd "$DIST" && pwd)/$BASENAME.AppImage"
  # Extract-and-run: works without FUSE (containers, CI); the output embeds the static runtime.
  ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream --runtime-file "$RUNTIME" "$APPDIR" "$OUT"
  echo "wrote $OUT"
fi

echo "==> done"
ls -lh "$DIST"
