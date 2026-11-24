#!/bin/sh
# Builds musiccraft-ffi (crates/ffi) as a static library for the platform Xcode is building:
# build/rust/<platform>/libmusiccraft_ffi.a. Needs rustup with the iOS targets:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
set -eu
export PATH="$HOME/.cargo/bin:$PATH"
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/../.."
platform="${PLATFORM_NAME:-iphonesimulator}"
case "$platform" in
  iphoneos) targets="aarch64-apple-ios" ;;
  *)
    targets=""
    for arch in ${ARCHS:-arm64}; do
      case "$arch" in
        arm64) targets="$targets aarch64-apple-ios-sim" ;;
        x86_64) targets="$targets x86_64-apple-ios" ;;
      esac
    done
    ;;
esac
out="$here/build/rust/$platform"
mkdir -p "$out"
libs=""
for target in $targets; do
  # Xcode's own settings would steer cargo's linker and SDK choice; cargo picks its own.
  env -u SDKROOT -u IPHONEOS_DEPLOYMENT_TARGET cargo build --release -p musiccraft-ffi --target "$target"
  libs="$libs target/$target/release/libmusiccraft_ffi.a"
done
# shellcheck disable=SC2086
lipo -create $libs -output "$out/libmusiccraft_ffi.a"
