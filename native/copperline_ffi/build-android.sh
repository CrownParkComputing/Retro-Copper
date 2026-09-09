#!/usr/bin/env bash
# Build the Copperline bridge as the Android core and put it where the app
# loads it from.
#
# The app dlopens "libuae4arm.so", so that is what this produces: the crate's
# [lib] name is uae4arm, and the twenty-five uae4arm_host_* functions it
# exports are the same ones the C++ Amiberry core exports. Which core the app
# runs is therefore decided here, by which .so lands in jniLibs.
#
#   native/copperline_ffi/build-android.sh            # arm64, release
#   ABI=armeabi-v7a native/copperline_ffi/build-android.sh
#
# Needs the Android NDK (ANDROID_NDK_HOME, or the newest under the SDK),
# cargo-ndk, and the rust target for the ABI.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
ABI="${ABI:-arm64-v8a}"
# minSdk in app/android/app/build.gradle.kts. The core is this app's own
# native code, so the platform level it targets is that same minimum.
API="${API:-28}"
JNILIBS="$REPO/app/android/app/src/main/jniLibs/$ABI"

if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    ANDROID_NDK_HOME=$(ls -d "$HOME"/Android/Sdk/ndk/* 2>/dev/null | sort -V | tail -1 || true)
fi
[ -n "$ANDROID_NDK_HOME" ] || { echo "error: no NDK; set ANDROID_NDK_HOME" >&2; exit 1; }
export ANDROID_NDK_HOME
command -v cargo-ndk >/dev/null || { echo "error: cargo install cargo-ndk" >&2; exit 1; }

# Copperline's block-device layer has backends for macOS, Linux and Windows
# and none named for Android, so the module is simply missing on a phone and
# the core does not compile. Android IS Linux - same sysfs, same ioctls - so
# the fix is one line, and it is carried here as a patch rather than as a
# fork: upstream is pinned by the submodule, and when it takes the change
# this file goes away. Applying it is idempotent.
patch_core() {
    local patch="$HERE/patches/0001-blockdev-android-uses-the-linux-backend.patch"
    [ -f "$patch" ] || return 0
    if git -C "$REPO/copperline" apply --reverse --check "$patch" 2>/dev/null; then
        echo "==> core patch already applied"
    else
        echo "==> applying the core's Android patch"
        git -C "$REPO/copperline" apply "$patch"
    fi
}

patch_core
echo "==> building $ABI (API $API) with NDK $(basename "$ANDROID_NDK_HOME")"
cd "$HERE"
cargo ndk -t "$ABI" --platform "$API" build --release

target_dir() {
    case "$ABI" in
        arm64-v8a) echo aarch64-linux-android ;;
        armeabi-v7a) echo armv7-linux-androideabi ;;
        x86_64) echo x86_64-linux-android ;;
        *) echo "error: unknown ABI $ABI" >&2; exit 1 ;;
    esac
}
BUILT="$HERE/target/$(target_dir)/release/libuae4arm.so"
[ -f "$BUILT" ] || { echo "error: no library at $BUILT" >&2; exit 1; }

# The count is checked rather than assumed: a library that loads but is
# missing an export fails at the first dlsym, deep inside the app, with a
# message that names one function and explains nothing.
exports=$(llvm-nm -D --defined-only "$BUILT" 2>/dev/null | grep -c "uae4arm_host_" || true)
[ "$exports" -eq 25 ] || {
    echo "error: $exports of 25 uae4arm_host_ functions exported" >&2
    exit 1
}

mkdir -p "$JNILIBS"
cp "$BUILT" "$JNILIBS/libuae4arm.so"
printf '==> %s (%s bytes, %s exports)\n' \
    "$JNILIBS/libuae4arm.so" "$(stat -c %s "$JNILIBS/libuae4arm.so")" "$exports"
