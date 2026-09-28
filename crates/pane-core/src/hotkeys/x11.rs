//! Global hotkeys on X11: a passive grab of each shortcut's key on the root
//! window (`XGrabKey`), so the X server sends its presses to Pane whichever
//! window has focus. A shortcut another client has grabbed already is
//! refused by the server (`BadAccess`), which is how a conflict with another
//! application shows. X11 needs no permission for this.
//!
//! The key is the key code whose first level (no Shift) produces the
//! shortcut's key on the current layout. A key reached only with Shift, such
//! as the digits of a French AZERTY layout, is refused for a shortcut
//! without Shift and grabbed as it is for one with Shift: the shortcut is
//! what the user presses. X11 matches a grab's modifiers exactly, so each
//! shortcut is also grabbed with every combination of Caps Lock, Num Lock
//! and Scroll Lock, whose modifier bits the server's modifier mapping gives.
//! When the keyboard or modifier mapping changes (`MappingNotify`), every
//! grab is released by the key code it was made with and made again for the
//! new mapping. Dropping the adapter releases its grabs and stops its thread.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, GrabMode, Keycode, ModMask,
    Window, WindowClass,
};
use x11rb::protocol::{ErrorKind, Event};
use x11rb::rust_connection::RustConnection;

use super::{HotkeyError, Hotkeys, PressSender, Shortcut};

/// XK_Num_Lock and XK_Scroll_Lock.
const NUM_LOCK: u32 = 0xff7f;
const SCROLL_LOCK: u32 = 0xff14;

/// The adapter for one X11 display.
pub struct X11Hotkeys {
    connection: Arc<RustConnection>,
    root: Window,
    shared: Arc<Mutex<Shared>>,
    /// A window of Pane's own that no one sees, to wake the event thread
    /// when the adapter is dropped.
    waker: Window,
    stopping: Arc<AtomicBool>,
}

/// The keyboard mapping and the grabs, used by both the adapter and its
/// event thread.
struct Shared {
    keymap: Keymap,
    grabs: HashMap<Shortcut, Grab>,
}

/// One grabbed shortcut: exactly what was grabbed, so that it is released
/// with the same key code and modifiers even after the mapping changed.
struct Grab {
    keycode: Keycode,
    /// The shortcut's modifier bits, without the lock variants.
    modifiers: u16,
    /// The lock bit combinations grabbed with it.
    variants: Vec<u16>,
    /// Whether its key is held, so that holding it reports one press.
    held: bool,
}

/// The server's keyboard and modifier mapping, as far as hotkeys need it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Keymap {
    first: Keycode,
    keysyms_per_keycode: usize,
    keysyms: Vec<u32>,
    /// The modifier bits of Num Lock and Scroll Lock; 0 if no modifier has
    /// them.
    num_lock: u16,
    scroll_lock: u16,
}

impl Keymap {
    fn read(connection: &RustConnection) -> Result<Keymap, String> {
        let setup = connection.setup();
        let (first, last) = (setup.min_keycode, setup.max_keycode);
        let mapping = connection
            .get_keyboard_mapping(first, last - first + 1)
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        let modifiers = connection
            .get_modifier_mapping()
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        let mut keymap = Keymap {
            first,
            keysyms_per_keycode: usize::from(mapping.keysyms_per_keycode.max(1)),
            keysyms: mapping.keysyms,
            num_lock: 0,
            scroll_lock: 0,
        };
        let per_modifier = usize::from(modifiers.keycodes_per_modifier()).max(1);
        keymap.num_lock = keymap.modifier_of(&modifiers.keycodes, per_modifier, NUM_LOCK);
        keymap.scroll_lock = keymap.modifier_of(&modifiers.keycodes, per_modifier, SCROLL_LOCK);
        Ok(keymap)
    }

    /// The keysyms of `keycode`.
    fn keysyms_of(&self, keycode: Keycode) -> &[u32] {
        let Some(index) = keycode.checked_sub(self.first) else {
            return &[];
        };
        self.keysyms
            .chunks(self.keysyms_per_keycode)
            .nth(usize::from(index))
            .unwrap_or(&[])
    }

    /// The modifier bit whose keys produce `keysym`, from the modifier
    /// mapping `keycodes` (eight modifiers, `per_modifier` key codes each);
    /// 0 if none does.
    fn modifier_of(&self, keycodes: &[Keycode], per_modifier: usize, keysym: u32) -> u16 {
        keycodes
            .chunks(per_modifier)
            .take(8)
            .position(|keys| {
                keys.iter()
                    .any(|&key| key != 0 && self.keysyms_of(key).contains(&keysym))
            })
            .map_or(0, |modifier| 1 << modifier)
    }

    /// The key code that produces `keysym`, and whether it needs Shift:
    /// first a key whose first level (no Shift) is `keysym`, else one whose
    /// second level (Shift) is.
    fn key_for(&self, keysym: u32) -> Option<(Keycode, bool)> {
        let keys = || {
            self.keysyms
                .chunks(self.keysyms_per_keycode)
                .enumerate()
                .map(|(index, keysyms)| (self.first.saturating_add(index as u8), keysyms))
        };
        keys()
            .find(|(_, keysyms)| keysyms.first() == Some(&keysym))
            .map(|(keycode, _)| (keycode, false))
            .or_else(|| {
                keys()
                    .find(|(_, keysyms)| keysyms.get(1) == Some(&keysym))
                    .map(|(keycode, _)| (keycode, true))
            })
    }

    /// Every combination of Caps Lock, Num Lock and Scroll Lock bits, once
    /// each, starting with none.
    fn lock_variants(&self) -> Vec<u16> {
        let locks = [u16::from(ModMask::LOCK), self.num_lock, self.scroll_lock];
        let mut variants: Vec<u16> = Vec::new();
        for mask in 0..8u16 {
            let variant = (0..3)
                .filter(|bit| mask & (1 << bit) != 0)
                .fold(0, |variant, bit| variant | locks[bit]);
            if !variants.contains(&variant) {
                variants.push(variant);
            }
        }
        variants
    }

    /// The modifier bits that decide a shortcut: Shift, Control, Alt (Mod1)
    /// and Super (Mod4), except any that is a lock here.
    fn decisive(&self) -> u16 {
        let modifiers = u16::from(ModMask::SHIFT | ModMask::CONTROL | ModMask::M1 | ModMask::M4);
        modifiers & !(self.num_lock | self.scroll_lock)
    }

    /// What to grab for `shortcut`: its key code and modifier bits, or why
    /// it cannot be grabbed on this layout.
    fn grab_for(&self, shortcut: &Shortcut) -> Result<(Keycode, u16), HotkeyError> {
        let refused = |reason: String| HotkeyError::Refused(reason);
        let wanted =
            keysym(shortcut.key()).ok_or_else(|| refused(format!("{shortcut} has no X11 key")))?;
        let (keycode, needs_shift) = self
            .key_for(wanted)
            .ok_or_else(|| refused(format!("the keyboard layout has no key for {shortcut}")))?;
        if needs_shift && !shortcut.shift() {
            return Err(refused(format!(
                "on this keyboard layout {shortcut} needs Shift; add Shift or choose another key"
            )));
        }
        Ok((keycode, modifiers(shortcut)))
    }
}

/// The modifier bits of `shortcut`.
fn modifiers(shortcut: &Shortcut) -> u16 {
    let mut mask = 0u16;
    if shortcut.shift() {
        mask |= u16::from(ModMask::SHIFT);
    }
    if shortcut.control() {
        mask |= u16::from(ModMask::CONTROL);
    }
    if shortcut.alt() {
        mask |= u16::from(ModMask::M1);
    }
    if shortcut.super_key() {
        mask |= u16::from(ModMask::M4);
    }
    mask
}

/// The X keysym of `key` (see `Shortcut::key`).
fn keysym(key: &str) -> Option<u32> {
    match key.as_bytes() {
        [c @ (b'a'..=b'z' | b'0'..=b'9')] => Some(u32::from(*c)),
        _ if key == "space" => Some(0x20),
        _ => {
            let number: u32 = key.strip_prefix('f')?.parse().ok()?;
            // XK_F1 is 0xffbe.
            (1..=12).contains(&number).then(|| 0xffbe + number - 1)
        }
    }
}

/// Grabs `keycode` with `modifiers` and each of `variants` on `root`. If one
/// is refused, those grabbed are released again.
fn grab(
    connection: &RustConnection,
    root: Window,
    keycode: Keycode,
    modifiers: u16,
    variants: &[u16],
) -> Result<(), HotkeyError> {
    for (done, variant) in variants.iter().enumerate() {
        let checked = connection
            .grab_key(
                false,
                root,
                ModMask::from(modifiers | variant),
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map_err(ReplyError::from)
            .and_then(|cookie| cookie.check());
        if let Err(error) = checked {
            ungrab(connection, root, keycode, modifiers, &variants[..done]);
            return Err(match error {
                ReplyError::X11Error(error) if error.error_kind == ErrorKind::Access => {
                    HotkeyError::Taken
                }
                error => HotkeyError::Refused(error.to_string()),
            });
        }
    }
    Ok(())
}

/// Releases what [`grab`] grabbed.
fn ungrab(
    connection: &RustConnection,
    root: Window,
    keycode: Keycode,
    modifiers: u16,
    variants: &[u16],
) {
    for variant in variants {
        if let Ok(cookie) = connection.ungrab_key(keycode, root, ModMask::from(modifiers | variant))
        {
            cookie.ignore_error();
        }
    }
    let _ = connection.flush();
}

impl X11Hotkeys {
    /// Connects to the X11 display `display` (such as `:0`) and starts the
    /// thread that receives the grabbed keys' presses.
    pub fn connect(display: &str, presses: PressSender) -> Result<X11Hotkeys, String> {
        let (connection, screen) =
            RustConnection::connect(Some(display)).map_err(|error| error.to_string())?;
        // Holding a key must not report it again and again: without this
        // the server repeats a press and a release while it is held.
        connection
            .xkb_use_extension(1, 0)
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        connection
            .xkb_per_client_flags(
                xkb::ID::USE_CORE_KBD.into(),
                xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
                xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT,
                Default::default(),
                Default::default(),
                Default::default(),
            )
            .map_err(|error| error.to_string())?
            .reply()
            .map_err(|error| error.to_string())?;
        let root = connection.setup().roots[screen].root;
        let waker = connection
            .generate_id()
            .map_err(|error| error.to_string())?;
        connection
            .create_window(
                0,
                waker,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new(),
            )
            .map_err(|error| error.to_string())?
            .check()
            .map_err(|error| error.to_string())?;
        let keymap = Keymap::read(&connection)?;
        let connection = Arc::new(connection);
        let shared = Arc::new(Mutex::new(Shared {
            keymap,
            grabs: HashMap::new(),
        }));
        let stopping = Arc::new(AtomicBool::new(false));
        let thread = Receiver {
            connection: connection.clone(),
            root,
            shared: shared.clone(),
            presses,
            stopping: stopping.clone(),
        };
        std::thread::Builder::new()
            .name("pane-hotkeys".into())
            .spawn(move || thread.run())
            .map_err(|error| error.to_string())?;
        Ok(X11Hotkeys {
            connection,
            root,
            shared,
            waker,
            stopping,
        })
    }

    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Hotkeys for X11Hotkeys {
    fn unavailable(&self) -> Option<String> {
        None
    }

    fn register(&self, shortcut: &Shortcut) -> Result<(), HotkeyError> {
        let mut shared = self.shared();
        if shared.grabs.contains_key(shortcut) {
            return Ok(());
        }
        let (keycode, modifiers) = shared.keymap.grab_for(shortcut)?;
        let variants = shared.keymap.lock_variants();
        grab(&self.connection, self.root, keycode, modifiers, &variants)?;
        shared.grabs.insert(
            shortcut.clone(),
            Grab {
                keycode,
                modifiers,
                variants,
                held: false,
            },
        );
        Ok(())
    }

    fn unregister(&self, shortcut: &Shortcut) {
        if let Some(grab) = self.shared().grabs.remove(shortcut) {
            ungrab(
                &self.connection,
                self.root,
                grab.keycode,
                grab.modifiers,
                &grab.variants,
            );
        }
    }
}

impl Drop for X11Hotkeys {
    /// Releases every grab and stops the event thread, which closes the
    /// connection.
    fn drop(&mut self) {
        let grabs: Vec<Grab> = self.shared().grabs.drain().map(|(_, grab)| grab).collect();
        for grab in grabs {
            ungrab(
                &self.connection,
                self.root,
                grab.keycode,
                grab.modifiers,
                &grab.variants,
            );
        }
        self.stopping.store(true, Ordering::SeqCst);
        let wake = ClientMessageEvent::new(32, self.waker, 0u32, [0u32; 5]);
        if let Ok(cookie) = self
            .connection
            .send_event(false, self.waker, EventMask::NO_EVENT, wake)
        {
            cookie.ignore_error();
        }
        let _ = self.connection.flush();
    }
}

/// The event thread's side of the adapter.
struct Receiver {
    connection: Arc<RustConnection>,
    root: Window,
    shared: Arc<Mutex<Shared>>,
    presses: PressSender,
    stopping: Arc<AtomicBool>,
}

impl Receiver {
    /// Reports each press of a grabbed shortcut and follows mapping changes,
    /// until the adapter is dropped or the connection closes.
    fn run(self) {
        while let Ok(event) = self.connection.wait_for_event() {
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            let mut shared = self.shared.lock().unwrap_or_else(|p| p.into_inner());
            match event {
                Event::KeyPress(event) => {
                    let state = u16::from(event.state) & shared.keymap.decisive();
                    let pressed = shared
                        .grabs
                        .iter_mut()
                        .find(|(_, grab)| grab.keycode == event.detail && grab.modifiers == state);
                    if let Some((shortcut, grab)) = pressed
                        && !grab.held
                    {
                        grab.held = true;
                        self.presses.send(shortcut.clone());
                    }
                }
                Event::KeyRelease(event) => {
                    for grab in shared.grabs.values_mut() {
                        if grab.keycode == event.detail {
                            grab.held = false;
                        }
                    }
                }
                Event::MappingNotify(_) => self.follow_mapping(&mut shared),
                _ => {}
            }
        }
    }

    /// Releases every grab as it was made and grabs it again for the new
    /// keyboard and modifier mapping. A shortcut the new layout cannot
    /// produce, or another client took meanwhile, stays released.
    fn follow_mapping(&self, shared: &mut Shared) {
        let Ok(keymap) = Keymap::read(&self.connection) else {
            return;
        };
        if keymap == shared.keymap {
            return;
        }
        let grabs: Vec<(Shortcut, Grab)> = shared.grabs.drain().collect();
        for (_, grab) in &grabs {
            ungrab(
                &self.connection,
                self.root,
                grab.keycode,
                grab.modifiers,
                &grab.variants,
            );
        }
        shared.keymap = keymap;
        for (shortcut, _) in grabs {
            let regrabbed = shared
                .keymap
                .grab_for(&shortcut)
                .and_then(|(keycode, modifiers)| {
                    let variants = shared.keymap.lock_variants();
                    grab(&self.connection, self.root, keycode, modifiers, &variants)?;
                    Ok(Grab {
                        keycode,
                        modifiers,
                        variants,
                        held: false,
                    })
                });
            match regrabbed {
                Ok(grab) => {
                    shared.grabs.insert(shortcut, grab);
                }
                Err(error) => {
                    eprintln!(
                        "Pane: the hotkey {shortcut} was released after the keyboard mapping changed: {error}"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XK_AMPERSAND: u32 = 0x26;

    /// A keymap of two keysyms per key from key code 10: `keys` in order.
    fn keymap(keys: &[[u32; 2]]) -> Keymap {
        Keymap {
            first: 10,
            keysyms_per_keycode: 2,
            keysyms: keys.iter().flatten().copied().collect(),
            num_lock: 0,
            scroll_lock: 0,
        }
    }

    #[test]
    fn a_key_is_the_one_that_produces_it_without_shift() {
        // Key 10 gives "!" and "1" with Shift (as "1" on some layouts),
        // key 11 gives "1" without Shift.
        let keymap = keymap(&[[0x21, u32::from(b'1')], [u32::from(b'1'), 0x21]]);
        assert_eq!(keymap.key_for(u32::from(b'1')), Some((11, false)));
    }

    #[test]
    fn a_digit_that_needs_shift_is_refused_without_shift_and_grabbed_with_it() {
        // French AZERTY: the "1" key gives "&" without Shift.
        let keymap = keymap(&[[XK_AMPERSAND, u32::from(b'1')]]);
        let without = Shortcut::parse("ctrl+alt+1").unwrap();
        assert!(matches!(
            keymap.grab_for(&without),
            Err(HotkeyError::Refused(reason)) if reason.contains("needs Shift")
        ));
        let with = Shortcut::parse("ctrl+alt+shift+1").unwrap();
        let shift = u16::from(ModMask::SHIFT);
        let (keycode, modifiers) = keymap.grab_for(&with).unwrap();
        assert_eq!(keycode, 10);
        assert_eq!(modifiers & shift, shift);
    }

    #[test]
    fn num_lock_and_scroll_lock_come_from_the_modifier_mapping() {
        // Key 10 is Num Lock, key 11 Scroll Lock, key 12 a letter.
        let keymap = keymap(&[[NUM_LOCK, 0], [SCROLL_LOCK, 0], [u32::from(b'a'), 0]]);
        // Eight modifiers of two key codes: Mod3 (bit 5) holds Num Lock,
        // Mod5 (bit 7) Scroll Lock.
        let mut keycodes = vec![0u8; 16];
        keycodes[5 * 2] = 10;
        keycodes[7 * 2 + 1] = 11;
        assert_eq!(keymap.modifier_of(&keycodes, 2, NUM_LOCK), 1 << 5);
        assert_eq!(keymap.modifier_of(&keycodes, 2, SCROLL_LOCK), 1 << 7);
        assert_eq!(keymap.modifier_of(&keycodes, 2, 0xffe1), 0);
    }

    #[test]
    fn every_lock_combination_is_grabbed_once() {
        let lock = u16::from(ModMask::LOCK);
        let mut keymap = keymap(&[]);
        keymap.num_lock = 1 << 4;
        keymap.scroll_lock = 1 << 7;
        let mut variants = keymap.lock_variants();
        variants.sort_unstable();
        let (num, scroll) = (1 << 4, 1 << 7);
        let mut expected = vec![
            0,
            lock,
            num,
            scroll,
            lock | num,
            lock | scroll,
            num | scroll,
            lock | num | scroll,
        ];
        expected.sort_unstable();
        assert_eq!(variants, expected);
        // Without Scroll Lock on any modifier, the combinations are not
        // repeated.
        keymap.scroll_lock = 0;
        assert_eq!(keymap.lock_variants(), vec![0, lock, num, lock | num]);
        // A lock on Mod4 does not decide a shortcut.
        keymap.num_lock = u16::from(ModMask::M4);
        assert_eq!(keymap.decisive() & u16::from(ModMask::M4), 0);
    }
}
