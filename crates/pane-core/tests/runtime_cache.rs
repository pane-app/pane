//! Compiled extension code is kept in a cache directory owned by the caller,
//! so a later runtime can reuse it instead of recompiling; a component the
//! runtime is told to forget is loaded again from its file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    // The test runs share one compiled-code cache (`PANE_TEST_CODE_CACHE`,
    // set in `.cargo/config.toml`); this test checks the caller's own.
    Runtime::ignore_shared_code_cache();
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

/// The first file or folder under `dir` whose name starts with `prefix`.
fn find(dir: &Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with(prefix) {
            Some(path)
        } else if path.is_dir() {
            find(&path, prefix)
        } else {
            None
        }
    })
}

#[test]
fn caches_kept_beside_the_compiled_code_outlive_its_clean_up() {
    // As in the test above, the caller's own cache is the one checked.
    Runtime::ignore_shared_code_cache();
    let cache = empty_dir("caches-beside-compiled-code");
    // What Pane keeps in its cache folder beside the compiled code: the
    // application icons' cache and the file index.
    let kept = [
        cache.join("application-icons").join("index.json"),
        cache.join("application-icons").join("0123.png"),
        cache.join("file-index").join("index.json"),
        cache.join("file-index").join("0.wal"),
    ];
    for file in &kept {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"kept").unwrap();
    }

    // Compiling a component keeps its code, which starts Wasmtime's clean
    // up of the folder it was given: there is no lock of an earlier one.
    let runtime = Runtime::start_with_cache(cache.clone()).unwrap();
    block_on(runtime.render(&sample())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let cleaned = loop {
        if let Some(lock) = find(&cache, ".cleanup") {
            break lock;
        }
        assert!(Instant::now() < deadline, "no clean up started");
        std::thread::sleep(Duration::from_millis(20));
    };
    // The clean up deletes what it does not recognize as soon as it holds
    // its lock: it is given time to finish.
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        for file in &kept {
            assert!(
                file.is_file(),
                "{} was deleted by the clean up of the compiled code ({})",
                file.display(),
                cleaned.display()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn code_an_earlier_pane_compiled_into_the_cache_folder_is_moved_into_the_codes_own() {
    Runtime::ignore_shared_code_cache();
    let cache = empty_dir("code-from-an-earlier-pane");
    // An earlier Pane gave Wasmtime the cache folder itself, which kept its
    // code in `modules`.
    let earlier = cache.join("modules").join("wasmtime").join("compiled");
    std::fs::create_dir_all(earlier.parent().unwrap()).unwrap();
    std::fs::write(&earlier, b"code").unwrap();

    let _runtime = Runtime::start_with_cache(cache.clone()).unwrap();

    assert!(!cache.join("modules").exists());
    assert_eq!(
        std::fs::read(cache.join("code/modules/wasmtime/compiled")).unwrap(),
        b"code"
    );
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
