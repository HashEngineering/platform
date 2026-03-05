#!/bin/bash
set -e

# Build rs-sdk-ffi as a shared library for the local host machine.
# Output:  target/release/librs_sdk_ffi.dylib  (macOS)
#          target/release/librs_sdk_ffi.so      (Linux)
#
# After running this script, execute the Gradle unit tests with:
#   ./gradlew :sdk:testDebugUnitTest

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
PROJECT_ROOT="$SCRIPT_DIR/../.."

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}Building rs-sdk-ffi for host platform...${NC}"

cargo build \
    --lib \
    --release \
    --package rs-sdk-ffi \
    --manifest-path "$PROJECT_ROOT/Cargo.toml"

OS="$(uname -s)"
case "$OS" in
    Darwin)
        LIB="$PROJECT_ROOT/target/release/librs_sdk_ffi.dylib"
        ;;
    Linux)
        LIB="$PROJECT_ROOT/target/release/librs_sdk_ffi.so"
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

echo -e "${GREEN}✓ Build complete:${NC} ${YELLOW}$LIB${NC}"
echo ""
echo "Run tests:"
echo "  cd packages/kotlin-sdk && ./gradlew :sdk:testDebugUnitTest"
