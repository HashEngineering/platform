#!/bin/bash
set -e

# Build the FFI shared library for the local host machine (for JVM unit tests).
# Usage: ./build_local.sh [--unified]
#
#   (default)   builds rs-sdk-ffi         -> target/release/librs_sdk_ffi.{dylib,so}
#   --unified   builds rs-unified-sdk-ffi -> target/release/librs_unified_sdk_ffi.{dylib,so}
#                                            (full SDK + key-wallet + platform-wallet + shielded)
#
# After running, execute the Gradle unit tests with:
#   ./gradlew :sdk:testDebugUnitTest                              # read-path lib
#   ./gradlew :sdk:testDebugUnitTest -PdashNativeLib=rs_unified_sdk_ffi   # unified lib

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
PROJECT_ROOT="$SCRIPT_DIR/../.."

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Parse args
UNIFIED=0
for arg in "$@"; do
    case $arg in
        --unified) UNIFIED=1 ;;
    esac
done

if [ "$UNIFIED" -eq 1 ]; then
    FFI_PACKAGE="rs-unified-sdk-ffi"
    FFI_LIB="rs_unified_sdk_ffi"
    FEATURES_ARG=(--features shielded)
else
    FFI_PACKAGE="rs-sdk-ffi"
    FFI_LIB="rs_sdk_ffi"
    FEATURES_ARG=()
fi

echo -e "${GREEN}Building $FFI_PACKAGE for host platform...${NC}"

cargo build \
    --lib \
    --release \
    --package "$FFI_PACKAGE" \
    "${FEATURES_ARG[@]}" \
    --manifest-path "$PROJECT_ROOT/Cargo.toml"

OS="$(uname -s)"
case "$OS" in
    Darwin)
        LIB="$PROJECT_ROOT/target/release/lib$FFI_LIB.dylib"
        ;;
    Linux)
        LIB="$PROJECT_ROOT/target/release/lib$FFI_LIB.so"
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

echo -e "${GREEN}✓ Build complete:${NC} ${YELLOW}$LIB${NC}"
echo ""
echo "Run tests:"
if [ "$UNIFIED" -eq 1 ]; then
    echo "  cd packages/kotlin-sdk && ./gradlew :sdk:testDebugUnitTest -PdashNativeLib=rs_unified_sdk_ffi"
else
    echo "  cd packages/kotlin-sdk && ./gradlew :sdk:testDebugUnitTest"
fi
