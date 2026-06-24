# Android build: TLS / OpenSSL

When cross-compiling `rs-sdk-ffi` (the `platform` flavor) for Android via
`build_platform_android.sh`, the build can fail with:

```
Could not find openssl via pkg-config:
pkg-config has not been configured to support cross-compilation.
...
Could not find directory of OpenSSL installation, and this `-sys` crate cannot
proceed without this knowledge.
$TARGET = aarch64-linux-android
openssl-sys = 0.9.117
warning: build failed
```

This document explains the root cause and the two ways to fix it.

## Root cause

`reqwest` (a direct dependency of both `rs-sdk-ffi` and
`rs-sdk-trusted-context-provider`) is the source of the problem. Its **default
feature set includes `default-tls`**, which on a Linux/Android target resolves to
the native TLS stack:

```
reqwest "default"
  └── default-tls
        └── native-tls          (Rust wrapper over the OS-native TLS library)
              └── openssl-sys    (on Linux/Android, native-tls == system OpenSSL)
                    └── needs a prebuilt libssl/libcrypto for the TARGET
```

`openssl-sys` is a *linking* (`-sys`) crate. Its `build.rs` must locate a prebuilt
`libssl`/`libcrypto` **for the architecture being compiled**:

1. via `pkg-config` — which explicitly refuses to cross-compile, then
2. via `OPENSSL_DIR` / `*_OPENSSL_DIR` env vars, then
3. it gives up and errors.

- **Host build (macOS/Linux dev machine):** a system OpenSSL exists → found → fine.
- **`aarch64-linux-android` / `x86_64-linux-android`:** there is no system OpenSSL,
  pkg-config bails on cross-compilation, no `OPENSSL_DIR` is set, and the Android
  NDK does not ship `libssl` → `build.rs` errors before any Rust code compiles.

### Feature-unification note

Cargo merges features across the whole graph into a **single** `reqwest` build.
Even if one crate requests rustls only, as long as *any* crate leaves reqwest's
defaults on, `default-tls` is union'd back in for everyone and `openssl-sys`
returns. Both crates depending on the shared `reqwest`
(`rs-sdk-ffi` + `rs-sdk-trusted-context-provider`) must agree.

> Note: an earlier comment in the manifest blamed "grovedb's build-dep reqwest."
> That is not the cause — the workspace uses `resolver = "2"`, which isolates
> build-dependency features, and build-dep `openssl-sys` would compile for the
> host, not the Android target. The Android-target `openssl-sys` comes straight
> from these two crates' normal `reqwest` dependency.

---

## Solution 1 — `default-features = false` (rustls only) — recommended

Drop reqwest's defaults so only the pure-Rust rustls stack remains. This removes
`native-tls` and `openssl-sys` from the Android graph entirely — there is no C
library to find, so it cross-compiles with zero external dependencies.

In **every** crate that depends on the shared `reqwest`
(`rs-sdk-ffi/Cargo.toml` and `rs-sdk-trusted-context-provider/Cargo.toml`):

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls-native-roots"] }
```

- `rustls` is pure Rust — no `-sys` crate, no system library, cross-compiles anywhere.
- `rustls-tls-native-roots` loads the **OS trust-store certificates** (via
  `rustls-native-certs`); it does **not** pull in native-tls/OpenSSL.

**Pros:** smallest/fastest; no OpenSSL compiled; single TLS stack in the binary.
**Con:** relies on feature unification — if a future dependency re-enables
reqwest's `default-tls`, `openssl-sys` silently returns and the build breaks again.

## Solution 2 — vendored OpenSSL

Keep reqwest's default features and instead make `openssl-sys` build OpenSSL from
bundled source (via `openssl-src`) using the NDK clang, so it no longer needs a
system library. Add to `rs-sdk-ffi/Cargo.toml`:

```toml
# Android only: openssl-sys is pulled in by reqwest's default-tls -> native-tls.
# The `vendored` feature compiles OpenSSL from source with the NDK clang instead
# of searching for a (nonexistent) cross-compiled system OpenSSL.
[target.'cfg(target_os = "android")'.dependencies]
openssl = { version = "0.10", features = ["vendored"] }
```

How it works: declaring `openssl` with `vendored` propagates the `vendored`
feature to `openssl-sys`; Cargo unifies that onto the single `openssl-sys`
instance; per `openssl-sys`' `build/main.rs`, with `vendored` present (and
`OPENSSL_NO_VENDOR` unset) it builds from source using the `CC_*`/`AR_*` env that
`build_native.sh` already exports for the target.

**Pros:** self-contained — works regardless of which crates enable reqwest's
`default-tls`.
**Con:** compiles OpenSSL from C source per ABI (slower build) and links **both**
TLS stacks (rustls + native-tls/OpenSSL) into each library.

---

## Measured comparison

Same source, both Android ABIs, `build_platform_android.sh`:

| ABI         | Solution 1 (rustls only) | Solution 2 (vendored OpenSSL) | Delta   |
|-------------|--------------------------|-------------------------------|---------|
| arm64-v8a   | 44.5 MB                  | 50.7 MB                       | +6.2 MB |
| x86_64      | 44.7 MB                  | 50.9 MB                       | +6.2 MB |

Both produce working `librs_sdk_ffi.so`. The +6.2 MB on Solution 2 is the
vendored OpenSSL plus the redundant second TLS stack.

## Recommendation

Use **Solution 1** unless you cannot guarantee that every crate sharing the
`reqwest` dependency stays on `default-features = false`. Solution 1 is smaller,
faster to build, and carries no OpenSSL. Reach for Solution 2 only when you need
robustness against a transitive dependency re-enabling reqwest's `default-tls`
outside your control.
