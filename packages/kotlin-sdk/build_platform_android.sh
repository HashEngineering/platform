#!/bin/bash
# Build the PLATFORM flavor (rs-sdk-ffi) Android .so files.
# Output: platform-sdk-android/src/main/jniLibs/<abi>/librs_sdk_ffi.so
# Usage: ./build_platform_android.sh [arm64|x86_64|all] [--clean]
set -e
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
exec "$SCRIPT_DIR/build_native.sh" platform --target android "$@"
