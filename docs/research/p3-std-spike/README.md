# Rust std on WASI 0.3: executable probe

2026-09-28. **Pass on Windows.** Unlike the earlier no_std probe, this uses ordinary Rust std for files, allocation/HashMap seeding, environment/arguments, wall and monotonic clocks, sleep, stdout and stderr.

The final component is 197,696 bytes and imports 17 interfaces, all `@0.3.0`. Successful read/write/readback/delete and no-preopen failure behavior passed in Wasmtime CLI 49.0.1. It also passed in the [P3-only embedded host](../p3-only-host/README.md), which rejects the old mixed P2/P3 JS component. [Results](result.json), [interface](component.wit), [source](src/main.rs).

The working toolchain is pinned `nightly-2026-09-27` (rustc 1.101.0-nightly, `75a75c3e0`) with source-built std, plus wasi-sdk 34. The SDK archive SHA-256 is `cccb5c323a9b34f0349a9b09e8804a0a7632c68c3310f4b5f437ed57d7e71d8f`. Both SDK and an isolated RUSTUP_HOME live under the scratch directory recorded in [local metadata](../p3-native-toolchain-local.json). The user's default toolchain was not changed.

The first Rust 1.97 build reached linking but used prerelease P3 bindings and the older calling convention. Selecting SDK 34's linker alone did not fix it; adding cooperative-threading linking exposed incompatible old object files. The pinned nightly, final P3 bindings, matching SDK linker, and `--cooperative-threading` produced the successful artifact. Historical build logs are available in Git history; rerunning the build generates a fresh local log. `RUSTC_BOOTSTRAP=1` was scoped to the initial failed stable experiment; the successful build uses nightly normally.

With the recorded scratch tools available, from the workspace root:

```powershell
python docs/research/p3-std-spike/build.py
python docs/research/p3-std-spike/run.py
```

The SDK/compiler are author/CI dependencies, not required end-user installations. This probe does not establish every Rust crate, native thread spawning, Tokio integration, other desktop platforms, or production extension lifecycle support.
