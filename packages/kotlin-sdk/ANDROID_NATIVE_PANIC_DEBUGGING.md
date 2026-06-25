# Debugging native Rust panics on Android (rs-sdk-ffi)

When an FFI call aborts the app with `SIGABRT`, the Android tombstone shows only
**where** it aborted, e.g.:

```
#03 rust_panic
#04 std::panicking::resume_unwind
#06 std::thread::scoped::scope
#07 dash_sdk_dpns_search+576      ← librs_sdk_ffi.so
#17 com.sun.jna.Native.invokeStructure
```

…but **not why**. The panic *message* goes to the default Rust panic hook, which
writes to **stderr** — and Android discards stderr. So you get the abort location,
never the cause.

> Context that makes this worse: the release profile sets `panic = "abort"`, and
> every FFI call runs its future on a dedicated big-stack worker thread (see
> `packages/rs-sdk-ffi/src/runtime.rs`). When that worker panics, `block_on`
> re-raises with `resume_unwind` and the process aborts. The hook below still runs
> at the original panic site (hooks run *before* the abort), so it captures the
> real message.

## The fix: a panic hook that writes to logcat

Temporarily add this to `packages/rs-sdk-ffi/src/lib.rs` (it routes the panic
message to Android logcat via `__android_log_write`, and still chains the default
stderr hook for host/desktop builds):

```rust
/// Install a process-wide panic hook that routes panic messages to a durable sink
/// (Android logcat via `__android_log_write`; the default stderr hook elsewhere).
/// Idempotent — safe to call from every SDK-create entry point.
pub(crate) fn install_panic_logger() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".to_string());
            let msg = match info.payload().downcast_ref::<&str>() {
                Some(s) => (*s).to_string(),
                None => match info.payload().downcast_ref::<String>() {
                    Some(s) => s.clone(),
                    None => "<non-string panic payload>".to_string(),
                },
            };
            let thread = std::thread::current();
            let thread_name = thread.name().unwrap_or("<unnamed>");
            let line = format!("thread '{thread_name}' panicked at {location}:\n{msg}");

            #[cfg(target_os = "android")]
            {
                // ANDROID_LOG_FATAL = 7. liblog is always linked on Android.
                extern "C" {
                    fn __android_log_write(
                        prio: std::os::raw::c_int,
                        tag: *const std::os::raw::c_char,
                        text: *const std::os::raw::c_char,
                    ) -> std::os::raw::c_int;
                }
                let tag = b"rs_sdk_ffi_panic\0";
                if let Ok(c_msg) = std::ffi::CString::new(line.clone()) {
                    unsafe {
                        __android_log_write(7, tag.as_ptr() as *const _, c_msg.as_ptr());
                    }
                }
            }

            // Always also run the default hook (stderr) so host/desktop builds and
            // any other sinks still see the panic.
            default_hook(info);
            let _ = &line; // used only on android; silence unused on other targets
        }));
    });
}
```

Then call it once, early — at the top of each SDK-create entry point in
`packages/rs-sdk-ffi/src/sdk.rs`:

```rust
pub unsafe extern "C" fn dash_sdk_create(config: *const DashSDKConfig) -> DashSDKResult {
    crate::install_panic_logger();
    // ...
}

pub unsafe extern "C" fn dash_sdk_create_trusted(config: *const DashSDKConfig) -> DashSDKResult {
    crate::install_panic_logger();
    // ...
}
```

## Rebuild, deploy, read

```bash
cd packages/kotlin-sdk
./build_platform_android.sh arm64          # or: all  (rebuilds librs_sdk_ffi.so)
./gradlew :username-search-app:installDebug # or your app module

# reproduce the crash, then:
adb logcat -d -s rs_sdk_ffi_panic
```

Example output that this exact hook surfaced (the bug it found — native TLS roots
missing on Android, fixed in `rs-dapi-client`):

```
F rs_sdk_ffi_panic: thread 'dash-sdk-ffi-block-on' panicked at
  packages/rs-dapi-client/src/transport/tonic_channel.rs:50:10:
F rs_sdk_ffi_panic: Failed to set TLS config:
  tonic::transport::Error(Transport, NativeCertsNotFound)
```

## Remove when done

This is **debug-only instrumentation** — it was intentionally *not* committed.
Delete `install_panic_logger` from `lib.rs` and the two `crate::install_panic_logger();`
call sites in `sdk.rs`, then rebuild. Verify nothing is left behind:

```bash
grep -rn 'install_panic_logger\|rs_sdk_ffi_panic' packages/rs-sdk-ffi/src/
```