#!/bin/bash
set -e

# Unified build engine for the Dash Kotlin SDK native libraries.
#
# Usage:
#   ./build_native.sh <platform|unified> [--target host|android] [arm64|x86_64|all] [--clean]
#
# Flavor (first positional arg, default: platform):
#   platform   builds rs-sdk-ffi         -> librs_sdk_ffi.{so,dylib}          (read-path SDK)
#   unified    builds rs-unified-sdk-ffi -> librs_unified_sdk_ffi.{so,dylib}  (full SDK + key-wallet
#                                            + platform-wallet + shielded)
#
# Target (--target, default: host):
#   host       builds the host shared library into <repo-root>/target/release/ (for JVM unit tests).
#   android    cross-compiles for Android via the NDK and copies .so files into the matching
#              flavor module's src/main/jniLibs/<abi>/.
#
# For android builds the arch selector (arm64|x86_64|all, default all) and --clean apply.
#
# Prefer the thin wrappers: build_platform_local.sh / build_platform_android.sh /
# build_unified_local.sh / build_unified_android.sh.
#
# Supported Android ABIs are 64-bit only: arm64-v8a (devices) and x86_64 (emulator).
# 32-bit ABIs (armeabi-v7a / x86) are intentionally dropped — the unified library's FFI
# structs carry hard-coded 64-bit size/alignment guards (matching the iOS aarch64-only
# framework), so 32-bit targets do not compile.

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
PROJECT_ROOT="$SCRIPT_DIR/../.."
HEADERS_OUT_DIR="$SCRIPT_DIR/native/include"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Parse arguments
FLAVOR=""
TARGET="host"
BUILD_ARCH="all"
CLEAN_BUILD=0

while [ $# -gt 0 ]; do
    case "$1" in
        platform|unified)
            FLAVOR="$1"
            ;;
        --target)
            shift
            TARGET="$1"
            ;;
        host|android)
            TARGET="$1"
            ;;
        arm64|x86_64|all)
            BUILD_ARCH="$1"
            ;;
        --clean)
            CLEAN_BUILD=1
            ;;
        *)
            echo -e "${RED}Unknown argument: $1${NC}"
            echo "Usage: ./build_native.sh <platform|unified> [--target host|android] [arm64|x86_64|all] [--clean]"
            exit 1
            ;;
    esac
    shift
done

# Default flavor is platform (preserves the historic read-path default).
FLAVOR="${FLAVOR:-platform}"

# Select crate / output library / features / android module by flavor.
if [ "$FLAVOR" = "unified" ]; then
    # Unified FFI crate: re-exports dash-network, key-wallet-ffi, rs-sdk-ffi and
    # platform-wallet-ffi into a single cdylib. key-wallet-ffi is an auto-pulled
    # Cargo git dependency — no separate checkout needed.
    FFI_PACKAGE="rs-unified-sdk-ffi"
    FFI_LIB_BASE="rs_unified_sdk_ffi"
    # Orchard / shielded-pool support is an opt-in Cargo feature; enable it for
    # parity with the iOS framework (which ships shielded by default).
    FFI_FEATURES="shielded"
    ANDROID_MODULE="unified-sdk-android"
elif [ "$FLAVOR" = "platform" ]; then
    FFI_PACKAGE="rs-sdk-ffi"
    FFI_LIB_BASE="rs_sdk_ffi"
    FFI_FEATURES=""
    ANDROID_MODULE="platform-sdk-android"
else
    echo -e "${RED}Unknown flavor: $FLAVOR. Use platform|unified${NC}"
    exit 1
fi

# cargo --features rejects an empty arg, so only pass it when non-empty.
FEATURES_ARG=()
if [ -n "$FFI_FEATURES" ]; then
    FEATURES_ARG=(--features "$FFI_FEATURES")
fi

# ---------------------------------------------------------------------------
# Host build
# ---------------------------------------------------------------------------
build_host() {
    echo -e "${GREEN}Building $FFI_PACKAGE ($FLAVOR flavor) for host platform...${NC}"

    cargo build \
        --lib \
        --release \
        --package "$FFI_PACKAGE" \
        "${FEATURES_ARG[@]}" \
        --manifest-path "$PROJECT_ROOT/Cargo.toml"

    local OS LIB
    OS="$(uname -s)"
    case "$OS" in
        Darwin) LIB="$PROJECT_ROOT/target/release/lib$FFI_LIB_BASE.dylib" ;;
        Linux)  LIB="$PROJECT_ROOT/target/release/lib$FFI_LIB_BASE.so" ;;
        *) echo -e "${RED}Unsupported OS: $OS${NC}"; exit 1 ;;
    esac

    echo -e "${GREEN}✓ Build complete:${NC} ${YELLOW}$LIB${NC}"
    echo ""
    echo "Run tests:"
    if [ "$FLAVOR" = "unified" ]; then
        echo "  ./gradlew :platform-sdk-jvm:test -PdashNativeLib=rs_unified_sdk_ffi"
    else
        echo "  ./gradlew :platform-sdk-jvm:test"
    fi
}

# ---------------------------------------------------------------------------
# Android build
# ---------------------------------------------------------------------------
build_android() {
    local FFI_LIB="lib$FFI_LIB_BASE.so"
    local ANDROID_PACKAGE_DIR="$SCRIPT_DIR/$ANDROID_MODULE/src/main/jniLibs"

    # Minimum Android API level (API 24 = Android 7.0)
    local ANDROID_API="${ANDROID_API:-24}"

    echo -e "${GREEN}Building $FFI_PACKAGE ($FFI_LIB) for Android ($BUILD_ARCH, API $ANDROID_API)${NC}"

    # Detect Android NDK
    if [ -z "$ANDROID_NDK_HOME" ]; then
        for candidate in \
            "$HOME/Library/Android/sdk/ndk/$(ls "$HOME/Library/Android/sdk/ndk/" 2>/dev/null | sort -V | tail -1)" \
            "$HOME/Android/Sdk/ndk/$(ls "$HOME/Android/Sdk/ndk/" 2>/dev/null | sort -V | tail -1)" \
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
    local HOST_TAG
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64) HOST_TAG="darwin-x86_64" ;;  # NDK prebuilt is x86_64 even on Apple Silicon (Rosetta)
        Darwin-x86_64) HOST_TAG="darwin-x86_64" ;;
        Linux-x86_64)  HOST_TAG="linux-x86_64" ;;
        Linux-aarch64) HOST_TAG="linux-aarch64" ;;
        *) echo -e "${RED}Unsupported host OS/arch: $(uname -s)-$(uname -m)${NC}"; exit 1 ;;
    esac

    local TOOLCHAIN="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG"
    if [ ! -d "$TOOLCHAIN" ]; then
        echo -e "${RED}NDK toolchain not found at $TOOLCHAIN${NC}"
        exit 1
    fi

    check_target() {
        if ! rustup target list --installed | grep -q "$1"; then
            echo -e "${YELLOW}Installing Rust target $1...${NC}"
            rustup target add "$1"
        fi
    }

    build_target() {
        local RUST_TARGET="$1"
        local ABI_DIR="$2"
        local CLANG_PREFIX="$3"

        check_target "$RUST_TARGET"

        local CC="$TOOLCHAIN/bin/${CLANG_PREFIX}${ANDROID_API}-clang"
        local CXX="$TOOLCHAIN/bin/${CLANG_PREFIX}${ANDROID_API}-clang++"
        local AR="$TOOLCHAIN/bin/llvm-ar"

        if [ ! -f "$CC" ]; then
            echo -e "${RED}Clang not found: $CC${NC}"
            return 1
        fi

        # Rust target env var format (replace - with _, uppercase; compatible with bash 3.x)
        local RUST_TARGET_ENV="${RUST_TARGET//-/_}"
        local RUST_TARGET_ENV_UPPER
        RUST_TARGET_ENV_UPPER="$(echo "$RUST_TARGET_ENV" | tr '[:lower:]' '[:upper:]')"

        echo -ne "${GREEN}Building $FFI_PACKAGE for $RUST_TARGET ($ABI_DIR)...${NC}"

        # NDK sysroot — gives C crates (e.g. rs-x11-hash) the Android headers.
        # cc-rs 1.x on Apple Silicon falls back to system clang when it can't find the NDK
        # prebuilt (it looks for darwin-arm64 which doesn't exist; NDK ships darwin-x86_64).
        # System clang can still cross-compile for Android when given the proper --sysroot.
        local SYSROOT="$TOOLCHAIN/sysroot"

        # Use `env` so the dynamically-named CARGO_TARGET_*_LINKER variable expands correctly.
        # Do NOT set generic CC/CXX — that would cause build scripts compiling HOST (macOS)
        # code (e.g. ring) to use the Android NDK clang, breaking host compilation.
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
               > "/tmp/cargo_build_${FLAVOR}_${ABI_DIR}.log" 2>&1; then
            echo -e "\r${GREEN}✓ $RUST_TARGET ($ABI_DIR) build successful${NC}        "
        else
            echo -e "\r${RED}✗ $RUST_TARGET ($ABI_DIR) build failed${NC}             "
            cat "/tmp/cargo_build_${FLAVOR}_${ABI_DIR}.log"
            return 1
        fi

        # Copy .so to the flavor module's jniLibs
        local OUT_DIR="$ANDROID_PACKAGE_DIR/$ABI_DIR"
        mkdir -p "$OUT_DIR"
        cp "$PROJECT_ROOT/target/$RUST_TARGET/release/$FFI_LIB" "$OUT_DIR/$FFI_LIB"
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

    case "$BUILD_ARCH" in
        arm64)
            build_target "aarch64-linux-android" "arm64-v8a" "aarch64-linux-android"
            ;;
        x86_64)
            build_target "x86_64-linux-android" "x86_64" "x86_64-linux-android"
            ;;
        all)
            build_target "aarch64-linux-android" "arm64-v8a" "aarch64-linux-android"
            build_target "x86_64-linux-android" "x86_64" "x86_64-linux-android"
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
    echo "  cd packages/kotlin-sdk && ./gradlew :$ANDROID_MODULE:assembleRelease"
}

case "$TARGET" in
    host)    build_host ;;
    android) build_android ;;
    *) echo -e "${RED}Unknown target: $TARGET. Use host|android${NC}"; exit 1 ;;
esac
