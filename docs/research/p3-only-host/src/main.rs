use wasmtime::{Config, Engine, Store, Error, Result, component::{Component, Linker, ResourceTable, Val}};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView, FsPerms};

struct State { wasi: WasiCtx, table: ResourceTable }
impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let path = args.get(1).ok_or_else(|| Error::msg("expected component path"))?;
    let mode = args.get(2).map(String::as_str).unwrap_or("command");
    let mut config = Config::new();
    config.wasm_component_model(true).wasm_component_model_async(true);
    let engine = Engine::new(&config)?;
    let component = Component::from_file(&engine, path)?;
    let mut linker = Linker::<State>::new(&engine);
    // Intentionally no P2 linker and no unknown-import stubs.
    wasmtime_wasi::p3::add_to_linker(&mut linker)?;
    let mut wasi = WasiCtxBuilder::new();
    wasi.inherit_stdout().inherit_stderr().args(&["probe", "success"])
        .env("P3_PROBE", "runtime-value");
    if let Some(dir) = args.get(4) {
        wasi.preopened_dir(dir, ".", FsPerms::ReadWrite)?;
    }
    let mut store = Store::new(&engine, State { wasi: wasi.build(), table: ResourceTable::new() });
    let instance = linker.instantiate_async(&mut store, &component).await?;
    if mode == "query" || mode == "queries" {
        let func = instance.get_func(&mut store, "query").ok_or_else(|| Error::msg("missing query export"))?;
        let input = args.get(3).cloned().unwrap_or_default();
        let count = if mode == "queries" { args.get(5).and_then(|s| s.parse().ok()).unwrap_or(3) } else { 1 };
        for _ in 0..count {
            let mut results = [Val::String(String::new())];
            let input = input.clone();
            store.run_concurrent(async |store| {
                func.call_concurrent(store, &[Val::String(input)], &mut results).await
            }).await??;
            if let Val::String(text) = &results[0] { println!("{text}"); }
        }
    } else {
        let command = wasmtime_wasi::p3::bindings::Command::new(&mut store, &instance)?;
        let result = store.run_concurrent(async |store| command.wasi_cli_run().call_run(store).await).await??;
        result.map_err(|()| Error::msg("guest command failed"))?;
    }
    Ok(())
}
