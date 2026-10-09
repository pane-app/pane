// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Pane's componentizer entry point, the binary a development build spawns
//! when it does not link the componentizer in-process (`p3_build <wit>
//! <world> <js> <runtime.wasm> <out.wasm>`, the contract #215's workflow
//! builds for every platform and `@pane-app/cli`'s platform packages will
//! carry). Requires `QJS_P3_LIBC` to name SDK 34's `wasm32-wasip3/libc.so`
//! (see `tools/componentize-js/patches`). It drives the componentize future
//! on a current-thread runtime of its own, as upstream's entry point does;
//! the `macros` feature is not needed for that, so `tokio` stays as light as
//! `pane-build`'s own use of it.
use componentize_qjs::{ComponentizeOpts, Runtime, componentize};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 6,
        "usage: p3_build <wit> <world> <js> <runtime.wasm> <out.wasm>"
    );
    let wit = PathBuf::from(&args[1]);
    let js = PathBuf::from(&args[3]);
    let runtime_wasm = std::fs::read(&args[4])?;
    let source = std::fs::read_to_string(&js)?;
    let start = std::time::Instant::now();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let component = runtime.block_on(componentize(&ComponentizeOpts {
        wit_path: &wit,
        js_source: &source,
        js_path: Some(&js),
        module_root: None,
        world_name: Some(&args[2]),
        stub_wasi: false,
        disable_gc: false,
        runtime: Runtime::Custom(&runtime_wasm),
        libc: None,
    }))?;
    std::fs::write(&args[5], &component)?;
    println!(
        "{{\"component_bytes\":{},\"componentize_ms\":{}}}",
        component.len(),
        start.elapsed().as_millis()
    );
    Ok(())
}
