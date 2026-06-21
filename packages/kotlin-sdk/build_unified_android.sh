#!/bin/bash
# Build the UNIFIED flavor (rs-unified-sdk-ffi) Android .so files.
# Output: unified-sdk-android/src/main/jniLibs/<abi>/librs_unified_sdk_ffi.so
# Usage: ./build_unified_android.sh [arm64|x86_64|all] [--clean]
set -e
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
exec "$SCRIPT_DIR/build_native.sh" unified --target android "$@"
