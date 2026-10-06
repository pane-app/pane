//! Compiled extension code is kept in a cache directory owned by the caller,
//! so a later runtime can reuse it instead of recompiling; a component the
//! runtime is told to forget is loaded again from its file.

use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::Runtime;

fn sample() -> PathBuf {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/guests/sample_rust.wasm");
    assert!(
        path.exists(),
        "{} is missing; run `cargo xtask guests`",
        path.display()
    );
    path
}

fn empty_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn files_in(dir: &PathBuf) -> usize {
    walk(dir)
}

fn walk(dir: &PathBuf) -> usize {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|entry| {
                if entry.path().is_dir() {
                    walk(&entry.path())
                } else {
                    1
                }
            })
            .sum()
    })
}

#[test]
fn compiled_code_is_cached_and_reused_by_a_later_runtime() {
    let cache = empty_dir("compiled-code-cache");

    let first = Runtime::start_with_cache(cache.clone()).unwrap();
    let view = block_on(first.render(&sample())).unwrap();
    assert!(
        files_in(&cache) > 0,
        "compiled code was written to {}",
        cache.display()
    );

    let second = Runtime::start_with_cache(cache.clone()).unwrap();
    assert_eq!(block_on(second.render(&sample())).unwrap(), view);
}

fn guest(name: &str) -> PathBuf {
    sample().with_file_name(format!("{name}.wasm"))
}

#[test]
fn a_forgotten_component_is_loaded_again_from_its_file() {
    let dir = empty_dir("forgotten-component");
    std::fs::create_dir_all(&dir).unwrap();
    let component = dir.join("command.wasm");
    std::fs::copy(guest("sample_rust"), &component).unwrap();
    let runtime = Runtime::start().unwrap();
    assert_eq!(
        block_on(runtime.render(&component)).unwrap().title,
        "Rust sample"
    );

    std::fs::copy(guest("sample_js"), &component).unwrap();
    runtime.forget([component.clone()]);

    assert_eq!(
        block_on(runtime.render(&component)).unwrap().title,
        "JavaScript sample"
    );
}
