use componentize_qjs::{ComponentizeOpts, Runtime, componentize};
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let wit = PathBuf::from(&args[1]);
    let js = PathBuf::from(&args[2]);
    let runtime = std::fs::read(&args[3])?;
    let source = std::fs::read_to_string(&js)?;
    let component = componentize(&ComponentizeOpts {
        wit_path: &wit, js_source: &source, js_path: Some(&js), module_root: None,
        world_name: Some("search-extension"), stub_wasi: false, disable_gc: false,
        runtime: Runtime::Custom(&runtime),
    }).await?;
    std::fs::write(&args[4], &component)?;
    println!("component_bytes={}", component.len());
    Ok(())
}
