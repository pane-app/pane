//! Background priority for the walker's and indexer's own threads (#126,
//! "Priority and safety valves"), so that an editor, a build or a game
//! never stutters because Pane is indexing. A query never runs at it.

/// Lowers the calling thread to background priority, for good: on Windows
/// background mode (low CPU, I/O and memory priority) and EcoQoS; on macOS
/// the background quality-of-service class (throttled I/O); on Linux the
/// lowest CPU priority and the idle I/O class. Returns whether the system
/// accepted it; indexing goes on at normal priority otherwise.
pub fn lower_current_thread() -> bool {
    imp::lower()
}

#[cfg(windows)]
mod imp {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadInformation, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        THREAD_POWER_THROTTLING_CURRENT_VERSION, THREAD_POWER_THROTTLING_EXECUTION_SPEED,
        THREAD_POWER_THROTTLING_STATE, ThreadPowerThrottling,
    };

    pub(super) fn lower() -> bool {
        // SAFETY: the calling thread's pseudo handle, which needs no closing.
        let background =
            unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
        let throttling = THREAD_POWER_THROTTLING_STATE {
            Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
        };
        // SAFETY: as above; the state lives for the call and its size is
        // given. Windows before 10 1709 refuses it, which costs nothing.
        let eco = unsafe {
            SetThreadInformation(
                GetCurrentThread(),
                ThreadPowerThrottling,
                (&raw const throttling).cast(),
                size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
            )
        };
        background.is_ok() && eco.is_ok()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    const QOS_CLASS_BACKGROUND: u32 = 0x09;

    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
    }

    pub(super) fn lower() -> bool {
        // SAFETY: changes only the calling thread's own class.
        unsafe { pthread_set_qos_class_self_np(QOS_CLASS_BACKGROUND, 0) == 0 }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    /// `IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT`.
    const IOPRIO_IDLE: libc::c_long = 3 << 13;
    const IOPRIO_WHO_PROCESS: libc::c_long = 1;

    pub(super) fn lower() -> bool {
        // SAFETY: plain system calls about the calling thread, which Linux
        // names by its thread id.
        unsafe {
            let thread = libc::syscall(libc::SYS_gettid);
            let nice = libc::setpriority(libc::PRIO_PROCESS, thread as libc::id_t, 19);
            let io = libc::syscall(
                libc::SYS_ioprio_set,
                IOPRIO_WHO_PROCESS,
                thread,
                IOPRIO_IDLE,
            );
            nice == 0 && io == 0
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    pub(super) fn lower() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_thread_of_its_own_can_be_lowered() {
        let lowered = std::thread::spawn(super::lower_current_thread)
            .join()
            .unwrap();
        // Every supported system accepts it for an ordinary thread.
        assert!(lowered);
    }
}
