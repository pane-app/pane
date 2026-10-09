//! Per-instance initialisation of engine state captured by the Wizer snapshot.
//!
//! QuickJS seeds `Math.random` (`ctx->random_state`) and records the
//! `performance` time origin when the context is created. Under componentize
//! that happens at build time, inside Wizer, so every instance of a component
//! would resume the same random sequence and measure `performance.now()` from
//! the build machine's monotonic clock. The context state is private to
//! QuickJS, so this module installs native `Math.random` and `performance`
//! replacements before any user code is evaluated, and re-seeds them the first
//! time the runtime is entered after the snapshot is restored.

use std::cell::Cell;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rquickjs::object::{Accessor, Property};
use rquickjs::{Ctx, Function, Object};

/// Plain `static` storage: under SDK 34's P3 libc each component-model task
/// gets its own TLS block, so Rust `thread_local!`s start from their initial
/// value on every export call and cannot hold instance-wide state.
struct Global<T>(Cell<T>);

// SAFETY: the runtime is single-threaded, as for `GlobalJsState`.
unsafe impl<T> Sync for Global<T> {}

impl<T: Copy> Global<T> {
    fn get(&self) -> T {
        self.0.get()
    }
    fn set(&self, value: T) {
        self.0.set(value)
    }
    fn replace(&self, value: T) -> T {
        self.0.replace(value)
    }
}

/// Set as the last step of Wizer initialisation, so it is `true` in the
/// snapshot and therefore on the first entry of every fresh instance.
static PENDING: Global<bool> = Global(Cell::new(false));
/// xorshift64* state; never zero once seeded.
static RANDOM_STATE: Global<u64> = Global(Cell::new(0));
static ORIGIN: Global<Option<Instant>> = Global(Cell::new(None));
static ORIGIN_WALL_MS: Global<f64> = Global(Cell::new(0.0));

unsafe extern "C" {
    /// wasi-libc; on wasm32-wasip3 backed by `wasi:random/random@0.3.0`.
    fn getentropy(buffer: *mut u8, len: usize) -> i32;
}

fn reseed() {
    let mut bytes = [0u8; 8];
    let status = unsafe { getentropy(bytes.as_mut_ptr(), bytes.len()) };
    assert_eq!(status, 0, "getentropy failed");
    RANDOM_STATE.set(u64::from_le_bytes(bytes).max(1));
    ORIGIN.set(Some(Instant::now()));
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    ORIGIN_WALL_MS.set(wall);
}

/// Same generator and mantissa construction as QuickJS's `js_math_random`.
fn math_random() -> f64 {
    let mut x = RANDOM_STATE.get();
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    RANDOM_STATE.set(x);
    let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
    f64::from_bits((0x3ff << 52) | (v >> 12)) - 1.0
}

fn performance_now() -> f64 {
    ORIGIN
        .get()
        .map(|origin| origin.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Replace the snapshot-sensitive builtins. Called once, before the shim and
/// user module are evaluated, so user code only ever sees the replacements.
pub(crate) fn install(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    reseed();
    let globals = ctx.globals();
    let math: Object = globals.get("Math")?;
    // Assignment keeps the builtin's writable/configurable/non-enumerable flags.
    math.set(
        "random",
        Function::new(ctx.clone(), math_random)?.with_name("random")?,
    )?;
    // QuickJS defines performance.timeOrigin as a non-configurable data
    // property, so the whole object is replaced (the global binding is
    // writable/configurable). timeOrigin becomes a getter, as in HR-Time.
    let performance = Object::new(ctx.clone())?;
    performance.prop(
        "now",
        Property::from(Function::new(ctx.clone(), performance_now)?.with_name("now")?)
            .enumerable(),
    )?;
    performance.prop(
        "timeOrigin",
        Accessor::new_get(|| ORIGIN_WALL_MS.get()).enumerable(),
    )?;
    globals.set("performance", performance)?;
    Ok(())
}

/// Mark the end of build-time initialisation; the next entry re-seeds.
pub(crate) fn arm() {
    PENDING.set(true);
}

/// Called on every runtime entry; re-seeds once per restored instance, before
/// any JavaScript runs in that instance.
pub(crate) fn on_entry() {
    if PENDING.replace(false) {
        reseed();
    }
}
