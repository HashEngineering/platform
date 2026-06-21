#!/bin/bash
# Build the PLATFORM flavor (rs-sdk-ffi) host library for JVM unit tests.
# Output: <repo-root>/target/release/librs_sdk_ffi.{dylib,so}
#   ./gradlew :platform-sdk-jvm:test
set -e
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
exec "$SCRIPT_DIR/build_native.sh" platform --target host "$@"
