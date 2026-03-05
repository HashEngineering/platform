@echo off
setlocal

:: Build rs-sdk-ffi as a shared library for the local Windows machine.
:: Output: target\release\rs_sdk_ffi.dll
::
:: After running this script, execute the Gradle unit tests with:
::   gradlew.bat :sdk:testDebugUnitTest

set SCRIPT_DIR=%~dp0
set PROJECT_ROOT=%SCRIPT_DIR%..\..

echo Building rs-sdk-ffi for Windows...

cargo build ^
    --lib ^
    --release ^
    --package rs-sdk-ffi ^
    --manifest-path "%PROJECT_ROOT%\Cargo.toml"

if %ERRORLEVEL% neq 0 (
    echo Build failed.
    exit /b 1
)

echo.
echo Build complete: %PROJECT_ROOT%\target\release\rs_sdk_ffi.dll
echo.
echo Run tests:
echo   cd packages\kotlin-sdk
echo   gradlew.bat :sdk:testDebugUnitTest
