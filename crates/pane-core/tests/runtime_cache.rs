//! Compiled extension code is kept in a cache directory owned by the caller,
//! so a later runtime can reuse it instead of recompiling.

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
    let view = block_on(first.get_view(&sample())).unwrap();
    assert!(
        files_in(&cache) > 0,
        "compiled code was written to {}",
        cache.display()
    );

    let second = Runtime::start_with_cache(cache.clone()).unwrap();
    assert_eq!(block_on(second.get_view(&sample())).unwrap(), view);
}
