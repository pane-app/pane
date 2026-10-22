//! Scratch componentizer entry point: `p3_build <wit> <world> <js> <runtime.wasm> <out.wasm>`.
//! Requires `QJS_P3_LIBC` to name SDK 34's `wasm32-wasip3/libc.so` (see runtime-port.patch).
use componentize_qjs::{ComponentizeOpts, Runtime, componentize};
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 6,
        "usage: p3_build <wit> <world> <js> <runtime.wasm> <out.wasm>"
    );
    let wit = PathBuf::from(&args[1]);
    let js = PathBuf::from(&args[3]);
    let runtime = std::fs::read(&args[4])?;
    let source = std::fs::read_to_string(&js)?;
    let start = std::time::Instant::now();
    let component = componentize(&ComponentizeOpts {
        wit_path: &wit,
        js_source: &source,
        js_path: Some(&js),
        module_root: None,
        world_name: Some(&args[2]),
        stub_wasi: false,
        disable_gc: false,
        runtime: Runtime::Custom(&runtime),
    })
    .await?;
    std::fs::write(&args[5], &component)?;
    println!(
        "{{\"component_bytes\":{},\"componentize_ms\":{}}}",
        component.len(),
        start.elapsed().as_millis()
    );
    Ok(())
}
