use std::time::Instant;
use wasmtime::{Config, Engine, Store, Error, Result, component::{Component, Linker, ResourceTable, Val}};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView, FsPerms};

struct State { wasi: WasiCtx, table: ResourceTable }
impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

fn ms(start: Instant) -> f64 { start.elapsed().as_secs_f64() * 1000.0 }

fn state(args: &[String]) -> Result<State> {
    let mut wasi = WasiCtxBuilder::new();
    wasi.inherit_stdout().inherit_stderr().args(&["probe", "success"])
        .env("P3_PROBE", "runtime-value");
    if let Some(dir) = args.get(4) {
        wasi.preopened_dir(dir, ".", FsPerms::ReadWrite)?;
    }
    Ok(State { wasi: wasi.build(), table: ResourceTable::new() })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let path = args.get(1).ok_or_else(|| Error::msg("expected component path"))?;
    let mode = args.get(2).map(String::as_str).unwrap_or("command");
    let mut config = Config::new();
    config.wasm_component_model(true).wasm_component_model_async(true);
    let engine = Engine::new(&config)?;
    let start = Instant::now();
    let component = if path.ends_with(".cwasm") {
        // SAFETY: only artifacts produced by this binary's `precompile` mode.
        unsafe { Component::deserialize_file(&engine, path)? }
    } else {
        Component::from_file(&engine, path)?
    };
    let load_ms = ms(start);
    if mode == "precompile" {
        let out = args.get(3).ok_or_else(|| Error::msg("expected output path"))?;
        std::fs::write(out, component.serialize()?)?;
        println!("{{\"compile_ms\":{load_ms:.3}}}");
        return Ok(());
    }
    let mut linker = Linker::<State>::new(&engine);
    // Intentionally no P2 linker and no unknown-import stubs.
    wasmtime_wasi::p3::add_to_linker(&mut linker)?;
    if mode == "instantiate" {
        // Fresh Store + instance per iteration; no export is called.
        let count: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);
        let pre = linker.instantiate_pre(&component)?;
        let mut samples = Vec::new();
        for _ in 0..count {
            let mut store = Store::new(&engine, state(&args)?);
            let start = Instant::now();
            pre.instantiate_async(&mut store).await?;
            samples.push(format!("{:.3}", ms(start)));
        }
        println!("{{\"load_ms\":{load_ms:.3},\"instantiate_ms\":[{}]}}", samples.join(","));
        return Ok(());
    }
    let mut store = Store::new(&engine, state(&args)?);
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
