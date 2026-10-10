//! The binding recognizer: the pure state machine that decides, from the
//! stream of key events, whether one of the bindings Pane bound was
//! pressed, and what a recording session recognized.
//!
//! It is compiled on every system and knows nothing of Windows: the
//! Windows half of the hotkey adapter feeds it the key events its
//! `WH_KEYBOARD_LL` hook sees ([`KeyEvent`]) and carries out what it
//! decides ([`Decision`]). The tests drive it with synthetic sequences on
//! every system, so the binding logic is checked without a Windows
//! machine.
//!
//! The rules match what `RegisterHotKey` does for the chords Windows
//! accepts, so a binding behaves the same whichever way it is
//! dispatched:
//!
//! - a chord fires when its key is pressed while exactly its modifiers
//!   are held, whatever keys were pressed between the modifiers and it;
//!   a modifier the binding names by a side is held on that side, and
//!   one it does not name is not held at all (#260);
//! - the key press that fires a chord, and its release, are swallowed, so
//!   the system does not also give the chord its own meaning; modifier
//!   presses and releases always pass through;
//! - a key pressed while already down is its auto-repeat: it fires
//!   nothing and repeats nothing;
//! - a lone tap of a modifier fires when it is released with no other key
//!   pressed between, within [`TAP_MS`] (#260); a double tap fires on the
//!   modifier's second press, with nothing between, within
//!   [`DOUBLE_MS`], and its first press passes through to applications,
//!   as every modifier's press and release does — or its state sticks
//!   for the system;
//! - a key Pane itself injected, tagged [`INJECTED_TAG`], is passed
//!   through untouched: Pane never reacts to its own input;
//! - a key another tool injected is recognized like the user's — a
//!   remapper sends the keys the user pressed, and the binding must work —
//!   but never swallowed: the key is the tool's, not Pane's to take. Such
//!   keys do not count as the user's either (they are not the
//!   keyboard's; see [`Recognizer::physical_events`]) — but the hook
//!   still sees them, which the liveness watchdog counts
//!   ([`Recognizer::events`]);
//! - while the Windows key is held, a key Pane took — the key of a chord
//!   it swallowed, or the second press of a double tap it fired — makes
//!   the Windows key's release carry [`Decision::Tap`] with its mask, or
//!   [`Decision::Mask`]: the adapter injects a tagged neutral key before
//!   the release reaches Windows, so the Start menu the release of a
//!   lone Windows key opens stays closed (the mask, #260). A Windows key
//!   released after a key Pane did not take passes through as it is, so
//!   Win+&lt;key&gt; chords keep Windows' meaning;
//! - a modifier event that contradicts the state Pane holds of the
//!   keyboard asks for the modifiers to be re-read from the system
//!   ([`Decision::Resync`]), as a session unlock and a resume do, so a
//!   release Pane missed never leaves a modifier stuck down for a
//!   phantom chord or up for a missed one.
//!
//! While a recording session listens, the same state machine runs in
//! recording mode ([`Recognizer::recording`]): every key event is held
//! back from the system — the Start menu the Windows key alone opens
//! stays closed — and each chord candidate (a key pressed with the
//! modifiers held, each any side or the one named) and each lone tap and
//! double tap recognized is reported ([`Recorded`]) for the recorder to
//! check as the window's own recorder checks a keystroke. Escape and Tab
//! never reach it: the adapter lets them through, so the recorder's own
//! cancellation keys work.
//!
//! One gap is deliberate: the first release of the Windows key of a
//! bound double tap — `double:win` without `tap:win` — passes through
//! unmasked, so the Start menu it opens opens; the tap that would mask
//! it is not bound, and nothing Pane could hold before the second press
//! would say whether one comes. The guard that refuses a single tap and
//! a double tap of the same modifier bound together keeps the two from
//! meeting in the recognizer; where a hand-edited record holds both
//! anyway, the release fires the tap and the second press the double,
//! each as it is recognized.

use super::{INJECTED_TAG, Kind, Side};

/// One key event, as the recognizer sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// The key, as a Windows virtual-key code — the codes the hook
    /// reports (`a` is 0x41, the left Windows key 0x5B).
    pub key: u32,
    /// The key's scan code, kept for the layout-independent keys later
    /// binding kinds need; the recognizer matches virtual-key codes.
    pub scan: u32,
    /// Whether the key was pressed or released.
    pub pressed: bool,
    /// Whether the key is one of the extended ones — the arrows and
    /// their neighbors, the numpad's Enter — as the hook reports it
    /// (`LLKHF_EXTENDED`). The numpad's Enter is told from the main one
    /// by this flag alone, their code being shared (#260).
    pub extended: bool,
    /// Whether a tool injected the event rather than the keyboard
    /// reporting it (`LLKHF_INJECTED` in the hook).
    pub injected: bool,
    /// The tag the injector left in the event's extra information: 0 for
    /// a key the keyboard reported, Pane's own [`INJECTED_TAG`] for the
    /// keys Pane injects.
    pub tag: usize,
    /// When the event happened, in milliseconds of the hook's clock: the
    /// tap and double-tap kinds are timed against it (#260); a chord
    /// needs no timing.
    pub time: u64,
}

/// What the recognizer decided about a key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// The event completes the chord of the binding it names (the id it
    /// was added with): the press is reported and the event swallowed, so
    /// the system does not also give the chord its own meaning. The key's
    /// release is swallowed too.
    Fire(u32),
    /// The event completes the chord of the binding it names, and a tool
    /// injected it rather than the keyboard: the press is reported — the
    /// binding works — but the event passes through, the key being the
    /// tool's, not Pane's to take. Pane's own tagged events are not this:
    /// they are passed through untouched, before any of this (see
    /// [`KeyEvent::tag`]).
    Injected(u32),
    /// The event completes the binding it names through the tap kinds
    /// (#260): the release of a modifier whose lone tap is bound, or the
    /// second press of one whose double tap is. The event passes
    /// through — a modifier's press and release must reach the system, or
    /// its state sticks for it — and `mask` says the adapter must first
    /// inject a tagged neutral key, so the system does not act on the
    /// modifier held alone: the release of the Windows key opens the
    /// Start menu, whatever Pane did with the keys between.
    Tap { binding: u32, mask: bool },
    /// The event passes through, and the adapter must first inject a
    /// tagged neutral key (#260): a Windows key is being released after
    /// Pane took a key pressed with it — the key of a chord it swallowed —
    /// and the release reaching Windows as a lone tap would open the
    /// Start menu.
    Mask,
    /// Swallow the event without reporting anything: the release or the
    /// auto-repeat of a key Pane swallowed the press of, or — while a
    /// recording session listens — a key event that recognized nothing to
    /// report (#260).
    Swallow,
    /// Let the event through to the system and the focused application.
    Pass,
    /// The event contradicts the state Pane holds of the keyboard — a
    /// modifier pressed while Pane holds it down, or released while Pane
    /// holds it up, which can only mean Pane missed events (the secure
    /// desktop of a lock or a UAC prompt had the keyboard). The caller
    /// re-reads the modifiers' real state from the system and hands it to
    /// [`Recognizer::resync`]. The event itself passes through.
    Resync,
    /// While a recording session listens (#260): the event was held back
    /// — swallowed — and the recognizer recognized something the recorder
    /// takes, as [`Recorded`] says.
    Recorded(Recorded),
}

/// What a recording session recognized from the keys the user pressed
/// (#260): a chord candidate — the modifiers held, each any side or the
/// one named, with the key — or a modifier pressed and released alone, or
/// twice with nothing between. The adapter names the keys and builds the
/// binding; whoever records checks it as the window's own recorder does
/// a keystroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recorded {
    /// A key pressed with these modifiers held (each: `None` for not
    /// held, [`Side::Any`] for either key, or the side named): a chord
    /// candidate, checked by whoever records it.
    Chord {
        /// Which of control, alt, shift and the Windows key were held,
        /// and on which side, in that order.
        modifiers: [Option<Side>; 4],
        /// The key's virtual-key code.
        key: u32,
        /// The numpad flag for the numpad's Enter, whose code the main
        /// Enter's is.
        numpad: bool,
    },
    /// A modifier pressed and released alone, within [`TAP_MS`], named by
    /// its position among the four — control, alt, shift, the Windows key
    /// — and the side of the key pressed.
    Tap { modifier: usize, side: Side },
    /// A modifier pressed twice with nothing between, within
    /// [`DOUBLE_MS`].
    Double { modifier: usize, side: Side },
}

/// The modifiers' positions as the recognizer holds them: control, alt,
/// shift, then the Windows key (Super on Linux, Command on macOS), as
/// [`super::Shortcut`] orders them.
const CONTROL: usize = 0;
const ALT: usize = 1;
const SHIFT: usize = 2;
const SUPER: usize = 3;

/// The modifier a virtual-key code is, as one of the recognizer's four
/// positions, and which of its two keys the code is: a left and a right
/// Ctrl are the same to a chord that does not name a side, and the
/// generic code an injected event can carry (`VK_CONTROL`) is that
/// modifier's left key, as the old tools send it. `None` for any other
/// key.
fn modifier(key: u32) -> Option<(usize, Physical)> {
    Some(match key {
        // VK_LSHIFT.
        0xA0 => (SHIFT, Physical::Left),
        // VK_RSHIFT.
        0xA1 => (SHIFT, Physical::Right),
        // VK_SHIFT, the generic code.
        0x10 => (SHIFT, Physical::Left),
        // VK_LCONTROL.
        0xA2 => (CONTROL, Physical::Left),
        // VK_RCONTROL.
        0xA3 => (CONTROL, Physical::Right),
        // VK_CONTROL, the generic code.
        0x11 => (CONTROL, Physical::Left),
        // VK_LMENU.
        0xA4 => (ALT, Physical::Left),
        // VK_RMENU.
        0xA5 => (ALT, Physical::Right),
        // VK_MENU, the generic code.
        0x12 => (ALT, Physical::Left),
        // VK_LWIN.
        0x5B => (SUPER, Physical::Left),
        // VK_RWIN.
        0x5C => (SUPER, Physical::Right),
        _ => return None,
    })
}

/// Which of a modifier's two keys an event was about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Physical {
    Left,
    Right,
}

impl Physical {
    /// The side the physical key is.
    fn side(self) -> Side {
        match self {
            Physical::Left => Side::Left,
            Physical::Right => Side::Right,
        }
    }
}

/// The virtual-key codes of the Windows key's two keys, whose releases
/// the recognizer masks (`VK_LWIN`, `VK_RWIN`).
fn is_windows(key: u32) -> bool {
    key == 0x5B || key == 0x5C
}

/// The main Enter's virtual-key code, which the numpad's Enter shares;
/// the numpad flag tells them apart (`VK_RETURN`).
const ENTER: u32 = 0x0D;

/// One binding as [`Recognizer::add`] takes it: what kind it is, which
/// modifiers (each any side or a named one, in the order control, alt,
/// shift, the Windows key), and the key — its virtual-key code, with the
/// numpad flag that tells the numpad's Enter from the main one (#260).
/// A tap or a double tap names exactly one modifier and holds no key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// What the binding is: a chord, a lone tap of a modifier, or a
    /// double tap of one.
    pub kind: Kind,
    /// Whether control, alt, shift and the Windows key are part of the
    /// binding, and which side of each it names, in that order: `None`
    /// for not part.
    pub modifiers: [Option<Side>; 4],
    /// The virtual-key code of a chord's key; 0 for the tap kinds.
    pub key: u32,
    /// The numpad flag for a chord ending with the numpad's Enter, whose
    /// code the main Enter's is.
    pub numpad: bool,
}

/// Whether a key event matches a binding's key: the same virtual-key
/// code, and, where the code is the Enter's — shared by the numpad's —
/// the same numpad flag (#260).
fn key_matches(event: &KeyEvent, binding: &Binding) -> bool {
    event.key == binding.key && (binding.key != ENTER || binding.numpad == event.extended)
}

/// Which of a modifier's two keys Pane last saw held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Held {
    /// Neither key.
    #[default]
    Neither,
    /// The left key.
    Left,
    /// The right key.
    Right,
    /// Both keys.
    Both,
}

impl Held {
    /// The state after `physical` is pressed. Pressing the other key
    /// while one is held — both keys of the modifier are down — serves
    /// either side's binding, as [`Held::side`] and the matching take
    /// `Held::Both`.
    fn pressed(self, physical: Physical) -> Held {
        match (self, physical) {
            (Held::Neither, Physical::Left) => Held::Left,
            (Held::Neither, Physical::Right) => Held::Right,
            _ => Held::Both,
        }
    }

    /// The state after `physical` is released.
    fn released(self, physical: Physical) -> Held {
        match (self, physical) {
            (Held::Left, Physical::Left) | (Held::Right, Physical::Right) => Held::Neither,
            (Held::Both, Physical::Left) => Held::Right,
            (Held::Both, Physical::Right) => Held::Left,
            (held, _) => held,
        }
    }

    /// Whether `physical`'s key is held.
    fn holds(self, physical: Physical) -> bool {
        matches!(
            (self, physical),
            (Held::Both, _) | (Held::Left, Physical::Left) | (Held::Right, Physical::Right)
        )
    }

    /// The side the held state reports for a chord candidate: both keys
    /// held is either side's press, as a binding that names no side
    /// takes.
    fn side(self) -> Option<Side> {
        match self {
            Held::Neither => None,
            Held::Left => Some(Side::Left),
            Held::Right => Some(Side::Right),
            Held::Both => Some(Side::Any),
        }
    }
}

/// Whether the modifiers held match a binding's: each the binding names
/// is held on a side it takes — either for [`Side::Any`], its own for a
/// named side — and each it does not name is not held at all.
fn modifiers_match(binding: &[Option<Side>; 4], held: &[Held; 4]) -> bool {
    binding.iter().zip(held).all(|(named, held)| {
        matches!(
            (named, held),
            (None, Held::Neither)
                | (Some(Side::Any), Held::Left | Held::Right | Held::Both)
                | (Some(Side::Left), Held::Left | Held::Both)
                | (Some(Side::Right), Held::Right | Held::Both)
        )
    })
}

/// One modifier's current press, for the tap and double-tap kinds: which
/// key was pressed, when, and whether another key was pressed while it
/// was held, which ends a tap (#260).
#[derive(Clone, Copy, Debug)]
struct Tap {
    /// The physical key pressed, as a virtual-key code.
    key: u32,
    /// When it was pressed, in milliseconds of the recognizer's clock.
    at: u64,
    /// Whether another key was pressed while it was held.
    broken: bool,
}

/// One modifier's last lone press, which a second press of the same key
/// within [`DOUBLE_MS`] completes as a double tap (#260).
#[derive(Clone, Copy, Debug)]
struct Double {
    /// The physical key pressed, as a virtual-key code; the second press
    /// must be the same key.
    key: u32,
    /// When it was pressed, in milliseconds of the recognizer's clock.
    at: u64,
}

/// How long a lone tap of a modifier may last: the press and its release
/// within this window, with no other key between, are a tap (#260). The
/// parent specification proposes 500 ms for the Windows key alone; one
/// window serves every modifier, so a tap feels the same whichever
/// modifier it is.
const TAP_MS: u64 = 500;

/// How long a double tap's two presses may be apart, with nothing between
/// them: 400 ms, as the parent specification proposes (#260).
const DOUBLE_MS: u64 = 400;

/// The state machine: the bindings Pane bound, the keyboard state it last
/// saw, the tap and double-tap candidacies it holds, and the key events
/// it counted for the liveness watchdog.
#[derive(Default)]
pub struct Recognizer {
    /// The bound bindings, in the order they were added. The hot path only
    /// reads this, and never allocates.
    bindings: Vec<(u32, Binding)>,
    /// Whether Pane last saw control, alt, shift and the Windows key
    /// held, and on which of each one's two keys.
    modifiers: [Held; 4],
    /// The non-modifier keys Pane last saw down, as a bitmap by
    /// virtual-key code, so a key pressed while already down is its
    /// auto-repeat.
    keys: [u64; 4],
    /// The key whose press fired a chord and whose release Pane still
    /// swallows, with its numpad flag. A key another tool injected never
    /// enters it: such keys are not swallowed.
    swallowed: Option<(u32, bool)>,
    /// Each modifier's current press, for the tap kinds (#260).
    taps: [Option<Tap>; 4],
    /// Each modifier's last lone press, for the double-tap kinds (#260).
    doubles: [Option<Double>; 4],
    /// Whether Pane took a key — swallowed one, or fired a binding —
    /// while a Windows key is held, so the Windows key's release is
    /// masked (#260).
    taken: bool,
    /// Whether a recording session listens (#260): every key event is
    /// held back and what the user presses is reported.
    recording: bool,
    /// How many key events the keyboard reported, injected ones apart.
    physical: u64,
    /// How many key events the hook saw, Pane's own injected keys and
    /// other tools' among them: the count the liveness watchdog compares
    /// the raw input with, so the keys a tool injects — which reach the
    /// raw input as the keyboard's do but are not the keyboard's — do not
    /// look like a hook Windows removed.
    events: u64,
}

impl Recognizer {
    /// Binds `binding` as binding `id`, which [`Decision::Fire`],
    /// [`Decision::Injected`] and [`Decision::Tap`] report when it is
    /// pressed.
    pub fn add(&mut self, id: u32, binding: Binding) {
        self.bindings.push((id, binding));
    }

    /// Unbinds the binding `id`.
    pub fn remove(&mut self, id: u32) {
        self.bindings.retain(|(bound, _)| *bound != id);
    }

    /// Whether no binding is bound, so the hook that watches the keyboard
    /// for them can be removed.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Turns the recording mode on or off (#260): while a session
    /// listens, every key event is held back from the system and what
    /// the user presses is reported as [`Decision::Recorded`]; the
    /// keyboard state starts afresh, as it does on a resync.
    pub fn recording(&mut self, on: bool) {
        if self.recording == on {
            return;
        }
        self.recording = on;
        self.forget();
    }

    /// One key event, and what to do about it.
    pub fn step(&mut self, event: KeyEvent) -> Decision {
        // Every event the hook saw, whatever its source: the watchdog's
        // evidence that the hook is still seeing the keyboard.
        self.events += 1;
        if event.tag == INJECTED_TAG {
            // Pane's own injected key: passed through untouched.
            return Decision::Pass;
        }
        if !event.injected {
            self.physical += 1;
        }
        let Some((at, physical)) = modifier(event.key) else {
            return self.step_key(event);
        };
        self.step_modifier(event, at, physical)
    }

    /// A non-modifier key event.
    fn step_key(&mut self, event: KeyEvent) -> Decision {
        let Some(word) = self.keys.get_mut((event.key / 64) as usize) else {
            // A virtual-key code past the bitmap: nothing binds it.
            return self.held_back();
        };
        let bit = 1u64 << (event.key % 64);
        if event.pressed {
            if *word & bit != 0 {
                // The key is already down: its auto-repeat, which fires
                // nothing and repeats nothing Pane swallowed. While a
                // recorder listens, its repeats are held back too.
                return if self.swallowed == Some((event.key, event.extended)) {
                    Decision::Swallow
                } else {
                    self.held_back()
                };
            }
            *word |= bit;
            // Another key pressed breaks every live tap and double-tap
            // candidacy (#260).
            self.break_taps();
            if self.recording {
                // A chord candidate for the recorder: the modifiers held,
                // each any side or the one named, and the key. The
                // recorder checks it as the window's own recorder checks
                // a keystroke.
                return Decision::Recorded(Recorded::Chord {
                    modifiers: self.modifiers.map(Held::side),
                    key: event.key,
                    numpad: event.extended,
                });
            }
            let Some(id) = self
                .bindings
                .iter()
                .find(|(_, binding)| {
                    binding.kind == Kind::Chord
                        && key_matches(&event, binding)
                        && modifiers_match(&binding.modifiers, &self.modifiers)
                })
                .map(|(id, _)| *id)
            else {
                return Decision::Pass;
            };
            if event.injected {
                return Decision::Injected(id);
            }
            self.swallowed = Some((event.key, event.extended));
            self.took();
            Decision::Fire(id)
        } else {
            *word &= !bit;
            if self.swallowed == Some((event.key, event.extended)) {
                self.swallowed = None;
                return Decision::Swallow;
            }
            self.held_back()
        }
    }

    /// A modifier event: the held state, the tap and double-tap
    /// candidacies, and the Start-menu mask.
    fn step_modifier(&mut self, event: KeyEvent, at: usize, physical: Physical) -> Decision {
        if self.modifiers[at].holds(physical) == event.pressed {
            // A modifier pressed while Pane holds that key down, or
            // released while Pane holds it up: events were missed while
            // they were not delivered to the hook, so the state is not
            // trusted.
            return Decision::Resync;
        }
        self.modifiers[at] = if event.pressed {
            self.modifiers[at].pressed(physical)
        } else {
            self.modifiers[at].released(physical)
        };
        if event.pressed {
            self.step_modifier_press(event, at, physical)
        } else {
            self.step_modifier_release(event, at, physical)
        }
    }

    /// A modifier press: every other candidacy breaks — another key was
    /// pressed — it may complete a double tap of the same key, and it
    /// begins its own tap candidacy.
    fn step_modifier_press(&mut self, event: KeyEvent, at: usize, physical: Physical) -> Decision {
        // The press is another key to every candidacy but the double tap
        // of this modifier it may complete, which is taken out first.
        let completing = self.doubles[at].filter(|double| double.key == event.key);
        self.break_taps();
        // The completing press of a double tap: the same key as the first
        // press, within the window, with nothing between. The first press
        // passed through; this one does too — a modifier's press must
        // reach the system, or its state sticks for it.
        if let Some(double) = completing
            && event.time.saturating_sub(double.at) <= DOUBLE_MS
        {
            if self.recording {
                return Decision::Recorded(Recorded::Double {
                    modifier: at,
                    side: physical.side(),
                });
            }
            if let Some(id) = self.tap_binding(at, physical.side(), Kind::Double) {
                // Pane took this press: the release that follows it is
                // masked.
                self.took();
                return Decision::Tap {
                    binding: id,
                    mask: false,
                };
            }
        }
        // The press begins its own tap candidacy.
        self.taps[at] = Some(Tap {
            key: event.key,
            at: event.time,
            broken: false,
        });
        if self.recording {
            return Decision::Swallow;
        }
        Decision::Pass
    }

    /// A modifier release: it may complete a lone tap, and it leaves the
    /// double-tap candidacy the next press of the same key may complete.
    fn step_modifier_release(
        &mut self,
        event: KeyEvent,
        at: usize,
        physical: Physical,
    ) -> Decision {
        let mut fired = None;
        let tap = self.taps[at].filter(|tap| tap.key == event.key);
        if let Some(tap) = tap {
            self.taps[at] = None;
            if !tap.broken && event.time.saturating_sub(tap.at) <= TAP_MS {
                // A lone press: its release completes a bound tap, and
                // leaves the candidacy a second press of the same key
                // completes as a double tap. The guard that refuses a
                // single and a double tap of the same modifier bound
                // together keeps the two from meeting, but a hand-edited
                // record may hold both, and each fires as it is
                // recognized.
                self.doubles[at] = Some(Double {
                    key: event.key,
                    at: tap.at,
                });
                if self.recording {
                    return Decision::Recorded(Recorded::Tap {
                        modifier: at,
                        side: physical.side(),
                    });
                }
                fired = self.tap_binding(at, physical.side(), Kind::Tap);
            }
        }
        // The Windows key's release is masked when Pane took a key under
        // it (#260): the Start menu the release of a lone Windows key
        // opens stays closed, whatever Pane did with the keys between.
        let windows = is_windows(event.key);
        let mask = windows && (self.taken || fired.is_some());
        // The last Windows key came up: the taken-key memory goes with it.
        if windows && self.modifiers[SUPER] == Held::Neither {
            self.taken = false;
        }
        match fired {
            Some(binding) => Decision::Tap { binding, mask },
            None if self.recording => Decision::Swallow,
            None if mask => Decision::Mask,
            None => Decision::Pass,
        }
    }

    /// The binding a tap of the modifier at `at`, on the side `side` of
    /// the key pressed, of `kind` names, if one is bound: a binding that
    /// names no side takes either key's tap.
    fn tap_binding(&self, at: usize, side: Side, kind: Kind) -> Option<u32> {
        self.bindings
            .iter()
            .find(|(_, binding)| {
                binding.kind == kind
                    && binding.modifiers[at]
                        .is_some_and(|named| named == Side::Any || named == side)
                    && binding
                        .modifiers
                        .iter()
                        .enumerate()
                        .all(|(other, named)| other == at || named.is_none())
            })
            .map(|(id, _)| *id)
    }

    /// Notes that Pane took a key while a Windows key is held, so the
    /// Windows key's release is masked.
    fn took(&mut self) {
        if self.modifiers[SUPER] != Held::Neither {
            self.taken = true;
        }
    }

    /// Breaks every live tap candidacy and forgets every double-tap
    /// candidacy: another key was pressed.
    fn break_taps(&mut self) {
        for tap in &mut self.taps {
            if let Some(tap) = tap.as_mut() {
                tap.broken = true;
            }
        }
        self.doubles = [None; 4];
    }

    /// Whether an event that recognizes nothing is swallowed — while a
    /// recording session listens — or passed through.
    fn held_back(&self) -> Decision {
        if self.recording {
            Decision::Swallow
        } else {
            Decision::Pass
        }
    }

    /// Takes the modifiers' real state as the system reports it, dropping
    /// the keyboard state Pane held: for a session unlock, a resume, the
    /// re-read a contradictory event asked for, and a recording session
    /// starting, so a release Pane missed never leaves a modifier down (a
    /// phantom chord) or up (a missed one). `down` names the modifier keys
    /// held, by their virtual-key codes.
    pub fn resync(&mut self, down: &[u32]) {
        self.modifiers = [Held::Neither; 4];
        for key in down {
            if let Some((at, physical)) = modifier(*key) {
                self.modifiers[at] = self.modifiers[at].pressed(physical);
            }
        }
        self.forget();
    }

    /// Drops the keyboard state beyond the modifiers: the keys down, the
    /// swallowed key, and the tap and double-tap candidacies.
    fn forget(&mut self) {
        self.keys = [0; 4];
        self.swallowed = None;
        self.taps = [None; 4];
        self.doubles = [None; 4];
        self.taken = false;
    }

    /// How many key events the keyboard reported since the recognizer was
    /// made, injected ones apart: the evidence the liveness watchdog
    /// compares with the raw input the keyboard delivers, so keys other
    /// tools inject are not counted as the user's.
    pub fn physical_events(&self) -> u64 {
        self.physical
    }

    /// How many key events the hook has seen, injected keys among them:
    /// the liveness watchdog's count (see [`Recognizer::events`] — the
    /// field's doc says what it is for).
    pub fn events(&self) -> u64 {
        self.events
    }
}
