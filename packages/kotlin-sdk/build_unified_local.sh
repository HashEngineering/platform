#!/bin/bash
# Build the UNIFIED flavor (rs-unified-sdk-ffi) host library for JVM unit tests.
# Output: <repo-root>/target/release/librs_unified_sdk_ffi.{dylib,so}
#   ./gradlew :platform-sdk-jvm:test -PdashNativeLib=rs_unified_sdk_ffi
set -e
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
exec "$SCRIPT_DIR/build_native.sh" unified --target host "$@"
