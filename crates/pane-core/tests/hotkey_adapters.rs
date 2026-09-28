//! Each system's global hotkey adapter against the real system: a shortcut
//! registers, a second registration of it (as another application would
//! make) is refused as taken, and releasing it, or dropping the adapter,
//! frees it for others.
//!
//! On Linux the checks run on a virtual X server of their own (Xvfb, from
//! `PANE_XVFB` or `PATH`), never on the desktop the tests run in, and a key
//! press made there with xdotool (`PANE_XDOTOOL` or `PATH`) is reported.
//! Without Xvfb they are skipped (CI's check job installs it). On
//! Windows the hotkeys are registered with the session the tests run in; a
//! press is made by the GUI smoke, not here. macOS registers hotkeys with the
//! main thread's run loop, which a test thread does not run, so the macOS
//! adapter is checked by the GUI smoke only.

#[cfg(any(target_os = "linux", target_os = "windows"))]
use pane_core::hotkeys::{HotkeyError, Hotkeys, Shortcut, channel};

#[cfg(target_os = "linux")]
mod x11 {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use pane_core::hotkeys::X11Hotkeys;

    /// A virtual X server, stopped when dropped.
    struct Xvfb {
        display: String,
        server: Child,
    }

    impl Drop for Xvfb {
        fn drop(&mut self) {
            let _ = self.server.kill();
            let _ = self.server.wait();
        }
    }

    fn tool(variable: &str, name: &str) -> Option<String> {
        if let Some(path) = std::env::var_os(variable) {
            return Some(path.to_string_lossy().into_owned());
        }
        let found = Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .ok()?;
        found
            .status
            .success()
            .then(|| String::from_utf8_lossy(&found.stdout).trim().to_owned())
    }

    /// Starts Xvfb on a display of its own, or `None` to skip where there is
    /// none.
    fn xvfb() -> Option<Xvfb> {
        let Some(xvfb) = tool("PANE_XVFB", "Xvfb") else {
            eprintln!("skipped: no Xvfb (set PANE_XVFB)");
            return None;
        };
        for _ in 0..20 {
            let number = 200 + (std::process::id() % 500) + fastrand_offset();
            let display = format!(":{number}");
            let server = Command::new(&xvfb)
                .args([&display, "-screen", "0", "640x480x24", "-nolisten", "tcp"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("Xvfb starts");
            let mut xvfb = Xvfb { display, server };
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if let Ok(Some(_)) = xvfb.server.try_wait() {
                    break; // the display was taken; try another
                }
                let socket = format!("/tmp/.X11-unix/X{number}");
                if std::path::Path::new(&socket).exists() {
                    std::thread::sleep(Duration::from_millis(200));
                    return Some(xvfb);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = xvfb.server.kill();
        }
        panic!("Xvfb did not start");
    }

    fn fastrand_offset() -> u32 {
        use std::time::SystemTime;
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        nanos % 300
    }

    #[test]
    fn a_grabbed_shortcut_is_reported_refused_to_others_and_released() {
        let Some(server) = xvfb() else {
            return;
        };
        let shortcut = Shortcut::parse("ctrl+alt+p").unwrap();
        let (sender, mut presses) = channel();
        let pane = X11Hotkeys::connect(&server.display, sender).expect("connects to Xvfb");
        assert_eq!(pane.unavailable(), None);
        pane.register(&shortcut).expect("registers");

        // Another client grabbing the same keys is refused.
        let (other_sender, _other_presses) = channel();
        let other = X11Hotkeys::connect(&server.display, other_sender).unwrap();
        assert_eq!(other.register(&shortcut), Err(HotkeyError::Taken));
        // A different shortcut is not affected.
        let different = Shortcut::parse("ctrl+alt+o").unwrap();
        other
            .register(&different)
            .expect("another shortcut registers");

        // A real key press on that display, whichever window has focus.
        if let Some(xdotool) = tool("PANE_XDOTOOL", "xdotool") {
            let pressed = Command::new(&xdotool)
                .args(["key", "ctrl+alt+p"])
                .env("DISPLAY", &server.display)
                .env_remove("WAYLAND_DISPLAY")
                .status()
                .expect("xdotool runs");
            assert!(pressed.success());
            let deadline = Instant::now() + Duration::from_secs(5);
            let reported = loop {
                if let Some(press) = presses.try_next() {
                    break Some(press);
                }
                if Instant::now() > deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            assert_eq!(reported, Some(shortcut.clone()), "the press is reported");
            assert_eq!(presses.try_next(), None, "one press is reported once");
        } else {
            eprintln!("no xdotool: the press is not checked");
        }

        // Released, the shortcut is free for another client.
        pane.unregister(&shortcut);
        other
            .register(&shortcut)
            .expect("the released shortcut registers elsewhere");
    }

    #[test]
    fn dropping_the_adapter_releases_its_grabs() {
        let Some(server) = xvfb() else {
            return;
        };
        let shortcut = Shortcut::parse("ctrl+alt+p").unwrap();
        let (sender, _presses) = channel();
        let pane = X11Hotkeys::connect(&server.display, sender).unwrap();
        pane.register(&shortcut).unwrap();
        // With Caps Lock, Num Lock and Scroll Lock variants too: another
        // client grabbing any of them is refused while Pane holds it.
        let (other_sender, _other_presses) = channel();
        let other = X11Hotkeys::connect(&server.display, other_sender).unwrap();
        assert_eq!(other.register(&shortcut), Err(HotkeyError::Taken));

        drop(pane);
        other
            .register(&shortcut)
            .expect("the dropped adapter's shortcut registers elsewhere");
    }

    #[test]
    fn a_display_that_cannot_be_reached_is_explained() {
        let (sender, _presses) = channel();
        let error = X11Hotkeys::connect(":4999", sender)
            .err()
            .expect("no X server on :4999");
        assert!(!error.is_empty());
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use pane_core::hotkeys::WindowsHotkeys;

    #[test]
    fn a_registered_shortcut_is_refused_to_others_and_released() {
        // An unlikely combination, so the session's own shortcuts are not
        // disturbed.
        let shortcut = Shortcut::parse("ctrl+alt+shift+f9").unwrap();
        let (sender, _presses) = channel();
        let pane = WindowsHotkeys::start(sender).expect("starts");
        assert_eq!(pane.unavailable(), None);
        pane.register(&shortcut).expect("registers");

        let (other_sender, _other_presses) = channel();
        let other = WindowsHotkeys::start(other_sender).unwrap();
        assert_eq!(other.register(&shortcut), Err(HotkeyError::Taken));

        pane.unregister(&shortcut);
        other
            .register(&shortcut)
            .expect("the released shortcut registers elsewhere");
        other.unregister(&shortcut);
    }

    #[test]
    fn dropping_the_adapter_releases_its_hotkeys() {
        let shortcut = Shortcut::parse("ctrl+alt+shift+f10").unwrap();
        let (sender, _presses) = channel();
        let pane = WindowsHotkeys::start(sender).unwrap();
        pane.register(&shortcut).unwrap();
        drop(pane);

        let (other_sender, _other_presses) = channel();
        let other = WindowsHotkeys::start(other_sender).unwrap();
        other
            .register(&shortcut)
            .expect("the dropped adapter's shortcut registers elsewhere");
    }
}
