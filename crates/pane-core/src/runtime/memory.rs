//! The memory each guest instance may use (#120): [`GUEST_MEMORY`], so one
//! misbehaving extension cannot make Pane heavy. A guest that asks for more
//! is refused it: its allocation fails, it traps, and the crash counts
//! towards pausing its package like any other. What the user is told says
//! that it ran out of memory and names the limit ([`out_of_memory`]),
//! rather than repeating the trap.

use wasmtime::ResourceLimiter;

use super::GuestState;

/// The most one linear memory of a guest instance may grow to: 128 MiB.
/// Web responses are capped well below it ([`crate::http::HttpLimits`]'s
/// 4 MiB body). It holds for each memory of the instance; a component
/// normally has one.
pub const GUEST_MEMORY: usize = 128 * 1024 * 1024;

/// Why a guest that asked for more than [`GUEST_MEMORY`] crashed, in place
/// of its trap's own message.
pub(crate) fn out_of_memory() -> String {
    format!(
        "it ran out of memory: an extension may use at most {} MiB",
        GUEST_MEMORY / (1024 * 1024)
    )
}

impl ResourceLimiter for GuestState {
    /// A memory may grow to [`GUEST_MEMORY`], and to the maximum it
    /// declares. Growing past the cap is refused, not trapped: the guest's
    /// allocator sees the failure (an engine may collect garbage and carry
    /// on), and the instance remembers it for the trap that usually follows.
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > GUEST_MEMORY {
            self.out_of_memory = true;
            return Ok(false);
        }
        let fits = maximum.is_none_or(|maximum| desired <= maximum);
        #[cfg(any(test, debug_assertions))]
        if fits {
            peaks::note(&self.component, desired);
        }
        Ok(fits)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(maximum.is_none_or(|maximum| desired <= maximum))
    }
}

/// The largest memory the instances of each component file had in this
/// process, by its file name (an installed package's copy counts with the
/// file it was copied from), for the tests that report how far under the
/// cap the samples and default extensions stay. Debug builds only.
#[cfg(any(test, debug_assertions))]
pub(super) mod peaks {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::{LazyLock, Mutex};

    static PEAKS: LazyLock<Mutex<HashMap<OsString, usize>>> = LazyLock::new(Mutex::default);

    /// Notes that an instance of `component` has a memory of `size` bytes.
    pub(in crate::runtime) fn note(component: &Path, size: usize) {
        let Some(name) = component.file_name() else {
            return;
        };
        let mut peaks = PEAKS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let peak = peaks.entry(name.to_owned()).or_default();
        *peak = (*peak).max(size);
    }

    /// The largest memory, in bytes, any instance of a component file named
    /// `file` (`sample_js.wasm`, say) had in this process, by any runtime;
    /// `None` if none ran.
    pub fn memory_peak(file: &str) -> Option<usize> {
        PEAKS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(std::ffi::OsStr::new(file))
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cap_is_128_mib_and_says_so() {
        assert_eq!(GUEST_MEMORY, 128 << 20);
        assert_eq!(
            out_of_memory(),
            "it ran out of memory: an extension may use at most 128 MiB"
        );
    }

    #[test]
    fn a_whole_web_response_stays_a_small_part_of_the_cap() {
        let body = crate::http::HttpLimits::default().body;
        assert_eq!(body, 4 << 20);
        assert!(body as usize * 16 <= GUEST_MEMORY);
    }
}
