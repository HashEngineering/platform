#!/bin/bash
set -e

# Build script for Dash SDK FFI (Android targets via NDK)
# This script builds the Rust library for Android targets and copies .so files
# Usage: ./build_android.sh [arm64|x86_64|all] [--unified] [--clean]
# Default: all supported ABIs, rs-sdk-ffi (read-path) library.
#
# Supported ABIs are 64-bit only: arm64-v8a (devices) and x86_64 (emulator).
# 32-bit ABIs (armeabi-v7a / x86) are intentionally dropped — the unified
# library's FFI structs carry hard-coded 64-bit size/alignment guards (matching
# the iOS aarch64-only framework), so 32-bit targets do not compile.
#
#   (default)   builds rs-sdk-ffi      -> librs_sdk_ffi.so          (preserves existing behavior)
#   --unified   builds rs-unified-sdk-ffi -> librs_unified_sdk_ffi.so (full SDK + wallet + shielded)
#
# Both libraries land side-by-side in jniLibs/<abi>/ (different file names), so a
# --unified build does NOT clobber the existing read-path library.

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
PROJECT_ROOT="$SCRIPT_DIR/../.."
ANDROID_PACKAGE_DIR="$SCRIPT_DIR/sdk/src/main/jniLibs"
HEADERS_OUT_DIR="$SCRIPT_DIR/native/include"

# Parse arguments
BUILD_ARCH="all"
CLEAN_BUILD=0
UNIFIED=0

for arg in "$@"; do
    case $arg in
        arm64|x86_64|all)
            BUILD_ARCH="$arg"
            ;;
        --unified)
            UNIFIED=1
            ;;
        --clean)
            CLEAN_BUILD=1
            ;;
    esac
done

# Select the crate / output library / features based on the build mode.
if [ "$UNIFIED" -eq 1 ]; then
    # Unified FFI crate: re-exports dash-network, key-wallet-ffi, rs-sdk-ffi and
    # platform-wallet-ffi into a single cdylib. key-wallet-ffi is an auto-pulled
    # Cargo git dependency — no separate checkout needed.
    FFI_PACKAGE="rs-unified-sdk-ffi"
    FFI_LIB="librs_unified_sdk_ffi.so"
    # Orchard / shielded-pool support is an opt-in Cargo feature; enable it for
    # parity with the iOS framework (which ships shielded by default).
    FFI_FEATURES="shielded"
else
    # Existing read-path library — unchanged.
    FFI_PACKAGE="rs-sdk-ffi"
    FFI_LIB="librs_sdk_ffi.so"
    FFI_FEATURES=""
fi

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Minimum Android API level (API 24 = Android 7.0)
ANDROID_API="${ANDROID_API:-24}"

echo -e "${GREEN}Building $FFI_PACKAGE ($FFI_LIB) for Android ($BUILD_ARCH, API $ANDROID_API)${NC}"

# Detect Android NDK
if [ -z "$ANDROID_NDK_HOME" ]; then
    # Common NDK locations
    for candidate in \
        "$HOME/Library/Android/sdk/ndk/$(ls $HOME/Library/Android/sdk/ndk/ 2>/dev/null | sort -V | tail -1)" \
        "$HOME/Android/Sdk/ndk/$(ls $HOME/Android/Sdk/ndk/ 2>/dev/null | sort -V | tail -1)" \
        "/usr/local/lib/android/sdk/ndk/$(ls /usr/local/lib/android/sdk/ndk/ 2>/dev/null | sort -V | tail -1)"; do
        if [ -d "$candidate" ]; then
            ANDROID_NDK_HOME="$candidate"
            break
        fi
    done
fi

if [ -z "$ANDROID_NDK_HOME" ] || [ ! -d "$ANDROID_NDK_HOME" ]; then
    echo -e "${RED}Error: ANDROID_NDK_HOME not set or not found.${NC}"
    echo "Please set ANDROID_NDK_HOME to your Android NDK directory."
    echo "Example: export ANDROID_NDK_HOME=\$HOME/Library/Android/sdk/ndk/26.3.11579264"
    exit 1
fi

echo "Using NDK: $ANDROID_NDK_HOME"

# Detect host OS and arch for NDK toolchain
case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) HOST_TAG="darwin-x86_64" ;;  # NDK prebuilt is x86_64 even on Apple Silicon (runs via Rosetta)
    Darwin-x86_64) HOST_TAG="darwin-x86_64" ;;
    Linux-x86_64)  HOST_TAG="linux-x86_64" ;;
    Linux-aarch64) HOST_TAG="linux-aarch64" ;;
    *) echo -e "${RED}Unsupported host OS/arch: $(uname -s)-$(uname -m)${NC}"; exit 1 ;;
esac

TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG"
if [ ! -d "$TOOLCHAIN" ]; then
    echo -e "${RED}NDK toolchain not found at $TOOLCHAIN${NC}"
    exit 1
fi

# Rust target to Android ABI mapping
declare_target() {
    local RUST_TARGET="$1"
    local ABI_DIR="$2"
    local CLANG_PREFIX="$3"

    echo "$RUST_TARGET $ABI_DIR $CLANG_PREFIX"
}

# Check and install required Rust targets
check_target() {
    if ! rustup target list --installed | grep -q "$1"; then
        echo -e "${YELLOW}Installing Rust target $1...${NC}"
        rustup target add "$1"
    fi
}

# Build one Android target
build_target() {
    local RUST_TARGET="$1"
    local ABI_DIR="$2"
    local CLANG_PREFIX="$3"

    check_target "$RUST_TARGET"

    local CC="$TOOLCHAIN/bin/${CLANG_PREFIX}${ANDROID_API}-clang"
    local CXX="$TOOLCHAIN/bin/${CLANG_PREFIX}${ANDROID_API}-clang++"
    local AR="$TOOLCHAIN/bin/llvm-ar"
    local RANLIB="$TOOLCHAIN/bin/llvm-ranlib"

    if [ ! -f "$CC" ]; then
        echo -e "${RED}Clang not found: $CC${NC}"
        return 1
    fi

    # Rust target env var format (replace - with _, uppercase via tr; compatible with bash 3.x)
    local RUST_TARGET_ENV="${RUST_TARGET//-/_}"
    local RUST_TARGET_ENV_UPPER
    RUST_TARGET_ENV_UPPER="$(echo "$RUST_TARGET_ENV" | tr '[:lower:]' '[:upper:]')"

    echo -ne "${GREEN}Building $FFI_PACKAGE for $RUST_TARGET ($ABI_DIR)...${NC}"

    # Only pass --features when non-empty (cargo rejects an empty --features arg).
    local FEATURES_ARG=()
    if [ -n "$FFI_FEATURES" ]; then
        FEATURES_ARG=(--features "$FFI_FEATURES")
    fi

    # NDK sysroot — gives C crates (e.g. rs-x11-hash) the Android headers.
    # cc-rs 1.x on Apple Silicon falls back to system clang when it can't find the NDK
    # prebuilt (it looks for darwin-arm64 which doesn't exist; NDK ships darwin-x86_64).
    # System clang can still cross-compile for Android when given the proper --sysroot.
    # cc-rs checks CFLAGS_<target_underscore> so we inject the sysroot there.
    local SYSROOT="$TOOLCHAIN/sysroot"

    # Use `env` so the dynamically-named CARGO_TARGET_*_LINKER variable expands correctly.
    # Do NOT set generic CC/CXX — that would cause build scripts that compile HOST (macOS)
    # code (e.g. ring) to use the Android NDK clang, breaking host compilation.
    # Target-specific CC_<target> and CFLAGS_<target> are used by cc-rs for cross builds.
    if env \
       "CC_${RUST_TARGET_ENV}=$CC" \
       "CXX_${RUST_TARGET_ENV}=$CXX" \
       "AR_${RUST_TARGET_ENV}=$AR" \
       "CFLAGS_${RUST_TARGET_ENV}=--sysroot=$SYSROOT" \
       "CXXFLAGS_${RUST_TARGET_ENV}=--sysroot=$SYSROOT" \
       "CARGO_TARGET_${RUST_TARGET_ENV_UPPER}_LINKER=$CC" \
       ANDROID_NDK="$ANDROID_NDK_HOME" \
       NDK_HOME="$ANDROID_NDK_HOME" \
       cargo build \
           --lib \
           --target "$RUST_TARGET" \
           --release \
           --package "$FFI_PACKAGE" \
           "${FEATURES_ARG[@]}" \
           --manifest-path "$PROJECT_ROOT/Cargo.toml" \
           > /tmp/cargo_build_${ABI_DIR}.log 2>&1; then
        echo -e "\r${GREEN}✓ $RUST_TARGET ($ABI_DIR) build successful${NC}        "
    else
        echo -e "\r${RED}✗ $RUST_TARGET ($ABI_DIR) build failed${NC}             "
        cat /tmp/cargo_build_${ABI_DIR}.log
        return 1
    fi

    # Copy .so to jniLibs
    local OUT_DIR="$ANDROID_PACKAGE_DIR/$ABI_DIR"
    mkdir -p "$OUT_DIR"
    cp "$PROJECT_ROOT/target/$RUST_TARGET/release/$FFI_LIB" \
       "$OUT_DIR/$FFI_LIB"
    echo -e "${GREEN}✓ Copied to $OUT_DIR/$FFI_LIB${NC}"

    # Collect cbindgen headers (emitted by each FFI crate's build.rs into the
    # per-target include dir) so the JNA-binding generator has a stable copy.
    local INCLUDE_SRC="$PROJECT_ROOT/target/$RUST_TARGET/release/include"
    if [ -d "$INCLUDE_SRC" ]; then
        mkdir -p "$HEADERS_OUT_DIR"
        cp -R "$INCLUDE_SRC/." "$HEADERS_OUT_DIR/" 2>/dev/null || true
    fi
}

if [ "$CLEAN_BUILD" -eq 1 ]; then
    echo -e "${GREEN}Cleaning Android build artifacts...${NC}"
    for target in aarch64-linux-android x86_64-linux-android; do
        cargo clean --release --target "$target" -p "$FFI_PACKAGE" --manifest-path "$PROJECT_ROOT/Cargo.toml" 2>/dev/null || true
    done
fi

# Build targets based on arch selection
case "$BUILD_ARCH" in
    arm64)
        build_target "aarch64-linux-android"  "arm64-v8a"  "aarch64-linux-android"
        ;;
    x86_64)
        build_target "x86_64-linux-android"   "x86_64"     "x86_64-linux-android"
        ;;
    all)
        build_target "aarch64-linux-android"   "arm64-v8a"  "aarch64-linux-android"
        build_target "x86_64-linux-android"    "x86_64"     "x86_64-linux-android"
        ;;
    *)
        echo -e "${RED}Unknown arch: $BUILD_ARCH. Use arm64|x86_64|all (64-bit only)${NC}"
        exit 1
        ;;
esac

echo -e "\n${GREEN}Build complete!${NC}"
echo -e "JNI libraries: ${YELLOW}$ANDROID_PACKAGE_DIR${NC}"
echo ""
echo "Now build the Android library:"
echo "  cd packages/kotlin-sdk && ./gradlew :sdk:assembleRelease"
