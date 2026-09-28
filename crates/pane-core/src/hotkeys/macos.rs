//! Global hotkeys on macOS: Carbon's `RegisterEventHotKey`, through the
//! `global-hotkey` crate. Presses arrive on the main run loop whichever
//! application is active. Carbon hot keys need no Accessibility or Input
//! Monitoring permission (unlike an event tap, which Pane does not use).
//! Carbon refuses only a shortcut another application registered
//! exclusively; Pane's own reserved list covers the system's shortcuts,
//! which it would not refuse.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use super::{HotkeyError, Hotkeys, PressSender, Shortcut};

/// Where presses go and which shortcut each registered id is. Carbon's
/// handler is installed once per process.
struct Routing {
    presses: Mutex<Option<PressSender>>,
    registered: Mutex<HashMap<u32, Shortcut>>,
}

static ROUTING: OnceLock<Arc<Routing>> = OnceLock::new();

/// The adapter.
pub struct MacHotkeys {
    manager: GlobalHotKeyManager,
    routing: Arc<Routing>,
}

fn code(key: &str) -> Option<Code> {
    Some(match key {
        "a" => Code::KeyA,
        "b" => Code::KeyB,
        "c" => Code::KeyC,
        "d" => Code::KeyD,
        "e" => Code::KeyE,
        "f" => Code::KeyF,
        "g" => Code::KeyG,
        "h" => Code::KeyH,
        "i" => Code::KeyI,
        "j" => Code::KeyJ,
        "k" => Code::KeyK,
        "l" => Code::KeyL,
        "m" => Code::KeyM,
        "n" => Code::KeyN,
        "o" => Code::KeyO,
        "p" => Code::KeyP,
        "q" => Code::KeyQ,
        "r" => Code::KeyR,
        "s" => Code::KeyS,
        "t" => Code::KeyT,
        "u" => Code::KeyU,
        "v" => Code::KeyV,
        "w" => Code::KeyW,
        "x" => Code::KeyX,
        "y" => Code::KeyY,
        "z" => Code::KeyZ,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        "f1" => Code::F1,
        "f2" => Code::F2,
        "f3" => Code::F3,
        "f4" => Code::F4,
        "f5" => Code::F5,
        "f6" => Code::F6,
        "f7" => Code::F7,
        "f8" => Code::F8,
        "f9" => Code::F9,
        "f10" => Code::F10,
        "f11" => Code::F11,
        "f12" => Code::F12,
        "space" => Code::Space,
        _ => return None,
    })
}

fn hot_key(shortcut: &Shortcut) -> Option<HotKey> {
    let mut modifiers = Modifiers::empty();
    if shortcut.control() {
        modifiers |= Modifiers::CONTROL;
    }
    if shortcut.alt() {
        modifiers |= Modifiers::ALT;
    }
    if shortcut.shift() {
        modifiers |= Modifiers::SHIFT;
    }
    if shortcut.super_key() {
        modifiers |= Modifiers::SUPER;
    }
    Some(HotKey::new(Some(modifiers), code(shortcut.key())?))
}

impl MacHotkeys {
    /// Installs the hot key handler. Call it on the main thread, whose run
    /// loop receives the presses.
    pub fn start(presses: PressSender) -> Result<MacHotkeys, String> {
        let routing = ROUTING
            .get_or_init(|| {
                let routing = Arc::new(Routing {
                    presses: Mutex::new(None),
                    registered: Mutex::new(HashMap::new()),
                });
                let handler = routing.clone();
                GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
                    if event.state != HotKeyState::Pressed {
                        return;
                    }
                    let shortcut = handler
                        .registered
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .get(&event.id)
                        .cloned();
                    let presses = handler.presses.lock().unwrap_or_else(|p| p.into_inner());
                    if let (Some(shortcut), Some(presses)) = (shortcut, presses.as_ref()) {
                        presses.send(shortcut);
                    }
                }));
                routing
            })
            .clone();
        *routing.presses.lock().unwrap_or_else(|p| p.into_inner()) = Some(presses);
        let manager = GlobalHotKeyManager::new().map_err(|error| error.to_string())?;
        Ok(MacHotkeys { manager, routing })
    }
}

impl Drop for MacHotkeys {
    /// Releases its hot keys and stops reporting presses. (The crate's
    /// manager removes Carbon's handler when it is dropped after this.)
    fn drop(&mut self) {
        let registered: Vec<Shortcut> = self
            .routing
            .registered
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .cloned()
            .collect();
        for shortcut in registered {
            self.unregister(&shortcut);
        }
        *self
            .routing
            .presses
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }
}

impl Hotkeys for MacHotkeys {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        let hot_key = hot_key(shortcut)
            .ok_or_else(|| HotkeyError::Refused(format!("{shortcut} has no macOS key")))?;
        let mut registered = self
            .routing
            .registered
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if registered.contains_key(&hot_key.id()) {
            return Ok(());
        }
        match self.manager.register(hot_key) {
            Ok(()) => {
                registered.insert(hot_key.id(), shortcut.clone());
                Ok(())
            }
            Err(global_hotkey::Error::AlreadyRegistered(_)) => Err(HotkeyError::Taken),
            Err(error) => Err(HotkeyError::Refused(error.to_string())),
        }
    }

    fn unregister(&self, shortcut: &Shortcut) {
        let Some(hot_key) = hot_key(shortcut) else {
            return;
        };
        let mut registered = self
            .routing
            .registered
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if registered.remove(&hot_key.id()).is_some() {
            let _ = self.manager.unregister(hot_key);
        }
    }
}
