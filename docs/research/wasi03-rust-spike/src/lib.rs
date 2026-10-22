#![no_std]
extern crate alloc;

use alloc::{string::ToString, vec::Vec};
use wasip3::filesystem::types::{DescriptorFlags, OpenFlags, PathFlags};

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

struct Probe;
wasip3::cli::command::export!(Probe);

impl wasip3::exports::cli::run::Guest for Probe {
    async fn run() -> Result<(), ()> {
        // Native WASI 0.3 async import; this actually suspends the guest.
        wasip3::clocks::monotonic_clock::wait_for(1_000_000).await;
        let dirs = wasip3::filesystem::preopens::get_directories();
        let (dir, _) = dirs.first().ok_or(())?;
        let file = dir.open_at(
            PathFlags::empty(), "fixture.json".to_string(),
            OpenFlags::empty(), DescriptorFlags::READ,
        ).await.map_err(|_| ())?;
        let (reader, finished) = file.read_via_stream(0);
        let bytes: Vec<u8> = reader.collect().await;
        finished.await.map_err(|_| ())?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
        if value["items"][0]["id"] != "rust-p3" { return Err(()); }
        let mut output = serde_json::to_vec(&value).map_err(|_| ())?;
        output.push(b'\n');
        let (mut writer, reader) = wasip3::wit_stream::new();
        let complete = wasip3::cli::stdout::write_via_stream(reader);
        let remaining = writer.write_all(output).await;
        drop(writer);
        if !remaining.is_empty() { return Err(()); }
        complete.await.map_err(|_| ())?;
        Ok(())
    }
}

// no_std on this installed target leaves the C byte-comparison symbol unresolved.
// Volatile loads keep LLVM from rewriting this implementation into a memcmp call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, len: usize) -> i32 {
    for i in 0..len {
        let x = unsafe { a.add(i).read_volatile() };
        let y = unsafe { b.add(i).read_volatile() };
        if x != y { return i32::from(x) - i32::from(y); }
    }
    0
}

// Normally supplied by the target's standard library; this no_std probe must
// export it explicitly. The canonical ABI passes valid allocation metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cabi_realloc(
    old: *mut u8, old_size: usize, align: usize, new_size: usize,
) -> *mut u8 {
    use alloc::alloc::{Layout, alloc, realloc};
    if new_size == 0 { return align as *mut u8; }
    let result = if old_size == 0 {
        unsafe { alloc(Layout::from_size_align_unchecked(new_size, align)) }
    } else {
        unsafe { realloc(old, Layout::from_size_align_unchecked(old_size, align), new_size) }
    };
    if result.is_null() { core::arch::wasm32::unreachable(); }
    result
}
