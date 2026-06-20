@echo off
setlocal

:: Build the FFI shared library for the local Windows machine (for JVM unit tests).
:: Usage: build_local.bat [--unified]
::
::   (default)   builds rs-sdk-ffi         -> target\release\rs_sdk_ffi.dll
::   --unified   builds rs-unified-sdk-ffi -> target\release\rs_unified_sdk_ffi.dll
::                                            (full SDK + key-wallet + platform-wallet + shielded)
::
:: After running, execute the Gradle unit tests with:
::   gradlew.bat :sdk:testDebugUnitTest
::   gradlew.bat :sdk:testDebugUnitTest -PdashNativeLib=rs_unified_sdk_ffi   (unified)

set SCRIPT_DIR=%~dp0
set PROJECT_ROOT=%SCRIPT_DIR%..\..

set FFI_PACKAGE=rs-sdk-ffi
set FFI_LIB=rs_sdk_ffi
set FEATURES=
if "%1"=="--unified" (
    set FFI_PACKAGE=rs-unified-sdk-ffi
    set FFI_LIB=rs_unified_sdk_ffi
    set FEATURES=--features shielded
)

echo Building %FFI_PACKAGE% for Windows...

cargo build ^
    --lib ^
    --release ^
    --package %FFI_PACKAGE% ^
    %FEATURES% ^
    --manifest-path "%PROJECT_ROOT%\Cargo.toml"

if %ERRORLEVEL% neq 0 (
    echo Build failed.
    exit /b 1
)

echo.
echo Build complete: %PROJECT_ROOT%\target\release\%FFI_LIB%.dll
echo.
echo Run tests:
echo   cd packages\kotlin-sdk
echo   gradlew.bat :sdk:testDebugUnitTest
