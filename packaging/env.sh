# shellcheck shell=bash
# Shared setup for the packaging scripts. Source it: `. "$(dirname "$0")/../env.sh"`.
# Adapted from storytold/craft-launcher (packaging/env.sh, MIT OR Apache-2.0).
#
# Exports:
#   ROOT              workspace root
#   VERSION           [workspace.package] version from Cargo.toml (override: MUSICCRAFT_VERSION)
#   DIST              output directory for release files (default: $ROOT/dist/release)
#   CARGO_TARGET_DIR  cargo's target dir (default: $ROOT/target)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") ; next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
}

VERSION="${MUSICCRAFT_VERSION:-$(workspace_version)}"
if [[ -z "$VERSION" ]]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$DIST"
export DIST
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

APP_ID=it.mauromazzieri.musiccraft
PKG=musiccraft
APP_NAME=MusicCraft
export APP_ID PKG APP_NAME

# A GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
}

# Licence, notice and readme files into a package directory.
copy_docs() {
  local dest="$1" f
  for f in README.md LICENSE-MIT LICENSE-APACHE NOTICE ATTRIBUTION.md THIRD-PARTY-LICENSES.txt; do
    if [[ -f "$ROOT/$f" ]]; then cp "$ROOT/$f" "$dest/"; fi
  done
}

check_sha256() {
  printf '%s  %s\n' "$1" "$2" | sha256sum -c -
}
