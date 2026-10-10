//! The binding recognizer (#252, #260) driven by synthetic key-event
//! sequences: the pure state machine the Windows half of the hotkey
//! adapter feeds the key events its low-level keyboard hook sees. It is
//! compiled and run on every system, so the binding logic is checked
//! without a Windows machine; its rules are the ones `RegisterHotKey`
//! keeps for the chords Windows accepts, so a binding behaves the same
//! whichever way it is dispatched.

use pane_core::hotkeys::{Binding, Decision, INJECTED_TAG, KeyEvent, Kind, Recorded, Side};

/// The virtual-key codes the tests press, as the hook reports them.
const CONTROL: u32 = 0xA2; // VK_LCONTROL
const RCONTROL: u32 = 0xA3; // VK_RCONTROL
const ALT: u32 = 0xA4; // VK_LMENU
const SHIFT: u32 = 0xA0; // VK_LSHIFT
const LWIN: u32 = 0x5B; // VK_LWIN
const RWIN: u32 = 0x5C; // VK_RWIN
const G: u32 = 0x47; // 'G'
const H: u32 = 0x48; // 'H'
const E: u32 = 0x45; // 'E'
const ENTER: u32 = 0x0D; // VK_RETURN, shared by the numpad's Enter

/// The modifiers as the recognizer's positions order them.
const CTRL_AT: usize = 0;
const ALT_AT: usize = 1;
const WIN_AT: usize = 3;

/// One key event, as the hook would report a physical key.
fn event(key: u32, pressed: bool) -> KeyEvent {
    KeyEvent {
        key,
        scan: 0,
        pressed,
        extended: false,
        injected: false,
        tag: 0,
        time: 0,
    }
}

/// One key event at `time`, as the hook would report it.
fn at(key: u32, pressed: bool, time: u64) -> KeyEvent {
    KeyEvent {
        time,
        ..event(key, pressed)
    }
}

/// A binding of the modifiers held (each either side) with `key`.
fn chord(modifiers: [bool; 4], key: u32) -> Binding {
    Binding {
        kind: Kind::Chord,
        modifiers: modifiers.map(|held| held.then_some(Side::Any)),
        key,
        numpad: false,
    }
}

/// A lone tap of the modifier at `at`, on `side`.
fn tap(at: usize, side: Side) -> Binding {
    let mut modifiers = [None; 4];
    modifiers[at] = Some(side);
    Binding {
        kind: Kind::Tap,
        modifiers,
        key: 0,
        numpad: false,
    }
}

/// A double tap of the modifier at `at`, on `side`.
fn double(at: usize, side: Side) -> Binding {
    let mut modifiers = [None; 4];
    modifiers[at] = Some(side);
    Binding {
        kind: Kind::Double,
        modifiers,
        key: 0,
        numpad: false,
    }
}

/// Presses `modifiers` down, then `key` down and up, then the modifiers
/// up, and returns the recognizer's decisions in order.
fn press(recognizer: &mut Recognizer, modifiers: &[u32], key: u32) -> Vec<Decision> {
    let mut events = Vec::new();
    for modifier in modifiers {
        events.push((*modifier, true));
    }
    events.push((key, true));
    events.push((key, false));
    for modifier in modifiers.iter().rev() {
        events.push((*modifier, false));
    }
    events
        .into_iter()
        .map(|(key, pressed)| recognizer.step(event(key, pressed)))
        .collect()
}

use pane_core::hotkeys::Recognizer;

/// Ctrl+Alt+G, bound as binding 1.
fn bound() -> Recognizer {
    let mut recognizer = Recognizer::default();
    recognizer.add(1, chord([true, true, false, false], G));
    recognizer
}

#[test]
fn a_chord_fires_when_its_key_is_pressed_with_its_modifiers_held() {
    let mut recognizer = bound();
    assert_eq!(
        press(&mut recognizer, &[CONTROL, ALT], G),
        vec![
            Decision::Pass,    // Ctrl down
            Decision::Pass,    // Alt down
            Decision::Fire(1), // G down: the chord, reported and swallowed
            Decision::Swallow, // G up: Pane swallowed its press
            Decision::Pass,    // Alt up
            Decision::Pass,    // Ctrl up
        ]
    );
}

#[test]
fn a_chord_needs_exactly_its_modifiers_and_its_key() {
    let mut recognizer = bound();
    // An extra modifier held makes another chord, as the system's own
    // registration treats it.
    for decision in press(&mut recognizer, &[CONTROL, ALT, SHIFT], G) {
        assert_eq!(decision, Decision::Pass);
    }
    // Another key with the modifiers held is not the chord.
    for decision in press(&mut recognizer, &[CONTROL, ALT], H) {
        assert_eq!(decision, Decision::Pass);
    }
}

#[test]
fn keys_pressed_between_the_modifiers_and_the_key_do_not_break_the_chord() {
    let mut recognizer = bound();
    let mut decisions = Vec::new();
    for (key, pressed) in [
        (CONTROL, true),
        (ALT, true),
        (H, true),
        (H, false),
        (G, true),
        (G, false),
        (ALT, false),
        (CONTROL, false),
    ] {
        decisions.push(recognizer.step(event(key, pressed)));
    }
    // The G between the modifiers and the key passed through, and the
    // chord fired anyway, as the system's registration would have.
    assert_eq!(decisions[2], Decision::Pass);
    assert_eq!(decisions[4], Decision::Fire(1));
}

#[test]
fn a_key_pressed_while_already_down_is_its_auto_repeat() {
    let mut recognizer = bound();
    recognizer.step(event(CONTROL, true));
    recognizer.step(event(ALT, true));
    assert_eq!(recognizer.step(event(G, true)), Decision::Fire(1));
    // Holding the key: its repeats fire nothing and repeat nothing, as
    // the registration's no-repeat does.
    assert_eq!(recognizer.step(event(G, true)), Decision::Swallow);
    assert_eq!(recognizer.step(event(G, true)), Decision::Swallow);
    assert_eq!(recognizer.step(event(G, false)), Decision::Swallow);
    // Pressed again once released: the chord fires again.
    assert_eq!(recognizer.step(event(G, true)), Decision::Fire(1));
}

#[test]
fn a_stuck_modifier_does_not_fire_a_phantom_chord_once_resynchronized() {
    let mut recognizer = bound();
    recognizer.step(event(CONTROL, true));
    recognizer.step(event(ALT, true));
    // The releases happened while the secure desktop of a lock or a UAC
    // prompt had the keyboard: Pane missed them. A session unlock
    // re-reads the modifiers' real state.
    recognizer.resync(&[]);
    assert_eq!(recognizer.step(event(G, true)), Decision::Pass);
    // The key was released, as a press is; the chord is pressed afresh.
    recognizer.step(event(G, false));
    // The modifiers really held after the re-read: the chord fires.
    recognizer.step(event(CONTROL, true));
    recognizer.step(event(ALT, true));
    assert_eq!(recognizer.step(event(G, true)), Decision::Fire(1));
}

#[test]
fn a_modifier_event_that_contradicts_the_state_asks_for_a_re_read() {
    let mut recognizer = Recognizer::default();
    recognizer.step(event(CONTROL, true));
    // Ctrl is held; a second press of it can only mean Pane missed its
    // release while the secure desktop had the keyboard.
    assert_eq!(recognizer.step(event(CONTROL, true)), Decision::Resync);
    // The caller re-reads the real state, and the keyboard goes on.
    recognizer.resync(&[CONTROL]);
    assert_eq!(recognizer.step(event(CONTROL, false)), Decision::Pass);
    assert_eq!(recognizer.step(event(CONTROL, true)), Decision::Pass);
}

#[test]
fn pane_s_own_injected_keys_are_passed_through_untouched() {
    let mut recognizer = bound();
    // The keys Pane itself injects — the Start-menu mask, a paste, a
    // simulated copy — carry Pane's tag: they are never the user's, and
    // never Pane's own bindings.
    let tagged = |key: u32, pressed: bool| KeyEvent {
        tag: INJECTED_TAG,
        ..event(key, pressed)
    };
    for (key, pressed) in [(CONTROL, true), (ALT, true), (G, true), (G, false)] {
        assert_eq!(recognizer.step(tagged(key, pressed)), Decision::Pass);
    }
    // Untouched: they left no state, so a real chord still fires.
    assert_eq!(
        press(&mut recognizer, &[CONTROL, ALT], G),
        vec![
            Decision::Pass,
            Decision::Pass,
            Decision::Fire(1),
            Decision::Swallow,
            Decision::Pass,
            Decision::Pass,
        ]
    );
    // And they were not the keyboard's, so the watchdog counted none.
    assert_eq!(recognizer.physical_events(), 6);
}

#[test]
fn keys_other_tools_inject_fire_the_binding_but_are_not_the_user_s_to_take() {
    let mut recognizer = bound();
    // A remapper — or the real-input adapter test, or the GUI smoke's
    // SendKeys — injects the chord with a tag of its own, not Pane's: the
    // keys the user pressed reach Pane, and the binding works.
    let injected = |key: u32, pressed: bool| KeyEvent {
        injected: true,
        ..event(key, pressed)
    };
    assert_eq!(recognizer.step(injected(CONTROL, true)), Decision::Pass);
    assert_eq!(recognizer.step(injected(ALT, true)), Decision::Pass);
    assert_eq!(recognizer.step(injected(G, true)), Decision::Injected(1));
    // The keys are the tool's, though, not Pane's to take: they pass
    // through, and their releases too.
    assert_eq!(recognizer.step(injected(G, false)), Decision::Pass);
    // They are not the user's either: the watchdog's evidence counts the
    // keyboard's events alone, so a hook that saw only injected keys
    // looks dead to it.
    assert_eq!(recognizer.physical_events(), 0);
    recognizer.step(event(H, true));
    assert_eq!(recognizer.physical_events(), 1);
}

#[test]
fn an_unbound_chord_no_longer_fires_and_an_empty_recognizer_needs_no_hook() {
    let mut recognizer = bound();
    assert!(!recognizer.is_empty());
    assert_eq!(
        press(&mut recognizer, &[CONTROL, ALT], G)[2],
        Decision::Fire(1)
    );
    recognizer.remove(1);
    assert!(recognizer.is_empty());
    // The chord fires no more.
    for decision in press(&mut recognizer, &[CONTROL, ALT], G) {
        assert_eq!(decision, Decision::Pass);
    }
}

#[test]
fn a_lone_tap_fires_when_its_modifier_is_released_with_nothing_between() {
    let mut recognizer = Recognizer::default();
    recognizer.add(1, tap(WIN_AT, Side::Any));
    // The press passes through, as every modifier's does; the release
    // within the window, with nothing between, fires the tap — and is
    // masked, the Start menu the Windows key's release would open stays
    // closed (#260).
    assert_eq!(recognizer.step(at(LWIN, true, 0)), Decision::Pass);
    assert_eq!(
        recognizer.step(at(LWIN, false, 100)),
        Decision::Tap {
            binding: 1,
            mask: true,
        }
    );
    // A tap of the other Windows key serves the same binding.
    assert_eq!(recognizer.step(at(RWIN, true, 200)), Decision::Pass);
    assert_eq!(
        recognizer.step(at(RWIN, false, 300)),
        Decision::Tap {
            binding: 1,
            mask: true,
        }
    );
}

#[test]
fn a_lone_tap_beyond_the_window_fires_nothing() {
    let mut recognizer = Recognizer::default();
    recognizer.add(1, tap(WIN_AT, Side::Any));
    assert_eq!(recognizer.step(at(LWIN, true, 0)), Decision::Pass);
    // 500 ms is the window's edge; past it, the press and release are a
    // key held, not a tap.
    assert_eq!(
        recognizer.step(at(LWIN, false, 500)),
        Decision::Tap {
            binding: 1,
            mask: true,
        }
    );
    assert_eq!(recognizer.step(at(LWIN, true, 10_000)), Decision::Pass);
    assert_eq!(recognizer.step(at(LWIN, false, 10_600)), Decision::Pass);
}

#[test]
fn a_key_pressed_between_the_press_and_the_release_breaks_the_tap() {
    let mut recognizer = Recognizer::default();
    recognizer.add(1, tap(WIN_AT, Side::Any));
    recognizer.step(at(LWIN, true, 0));
    // Another key pressed while the Windows key is held — one Pane does
    // not bind — breaks the tap, and the release passes as it is, so
    // Win+<key> keeps Windows' meaning (#260).
    assert_eq!(recognizer.step(at(E, true, 50)), Decision::Pass);
    assert_eq!(recognizer.step(at(E, false, 80)), Decision::Pass);
    assert_eq!(recognizer.step(at(LWIN, false, 100)), Decision::Pass);
}

#[test]
fn a_tap_bound_to_one_side_fires_only_on_that_side_s_key() {
    let mut recognizer = Recognizer::default();
    recognizer.add(1, tap(CTRL_AT, Side::Right));
    // The left Ctrl's tap is not the binding's; its release opens
    // nothing (and is not masked: only the Windows key's releases are).
    assert_eq!(recognizer.step(at(CONTROL, true, 0)), Decision::Pass);
    assert_eq!(recognizer.step(at(CONTROL, false, 100)), Decision::Pass);
    // The right Ctrl's tap is.
    assert_eq!(recognizer.step(at(RCONTROL, true, 200)), Decision::Pass);
    assert_eq!(
        recognizer.step(at(RCONTROL, false, 300)),
        Decision::Tap {
            binding: 1,
            mask: false,
        }
    );
}

#[test]
fn a_double_tap_fires_on_its_second_press_within_the_window() {
    let mut recognizer = Recognizer::default();
    recognizer.add(1, double(CTRL_AT, Side::Any));
    // The first press and its release pass through to applications, as
    // the specification says; the second press, with nothing between and
    // within the window, fires the binding. The second release passes
    // (a modifier's release must reach the system).
    assert_eq!(recognizer.step(at(CONTROL, true, 0)), Decision::Pass);
    assert_eq!(recognizer.step(at(CONTROL, false, 100)), Decision::Pass);
    assert_eq!(
        recognizer.step(at(CONTROL, true, 300)),
        Decision::Tap {
            binding: 1,
            mask: false,
        }
    );
    assert_eq!(recognizer.step(at(CONTROL, false, 400)), Decision::Pass);
}

#[test]
fn a_double_tap_beyond_the_window_or_of_another_key_fires_nothing() {
    let mut recognizer = Recognizer::default();
    recognizer.add(1, double(CTRL_AT, Side::Any));
    // 400 ms is the window's edge from the first press; past it, the two
    // presses are two taps.
    recognizer.step(at(CONTROL, true, 0));
    recognizer.step(at(CONTROL, false, 100));
    assert_eq!(recognizer.step(at(CONTROL, true, 500)), Decision::Pass);
    recognizer.step(at(CONTROL, false, 550));
    // The right Ctrl's press is another key between: the double of the
    // left one breaks, and a double bound to either side does not
    // complete across keys.
    recognizer.step(at(RCONTROL, true, 600));
    recognizer.step(at(RCONTROL, false, 700));
    assert_eq!(recognizer.step(at(CONTROL, true, 800)), Decision::Pass);
    // A double tap bound to one side completes on that side alone.
    let mut sided = Recognizer::default();
    sided.add(1, double(CTRL_AT, Side::Right));
    sided.step(at(RCONTROL, true, 0));
    sided.step(at(RCONTROL, false, 100));
    assert_eq!(
        sided.step(at(RCONTROL, true, 200)),
        Decision::Tap {
            binding: 1,
            mask: false,
        }
    );
}

#[test]
fn a_key_between_the_two_presses_breaks_the_double_tap() {
    let mut recognizer = Recognizer::default();
    recognizer.add(1, double(CTRL_AT, Side::Any));
    recognizer.step(at(CONTROL, true, 0));
    recognizer.step(at(CONTROL, false, 100));
    // Another key pressed between the two presses: the double does not
    // complete.
    assert_eq!(recognizer.step(at(H, true, 150)), Decision::Pass);
    assert_eq!(recognizer.step(at(H, false, 180)), Decision::Pass);
    assert_eq!(recognizer.step(at(CONTROL, true, 300)), Decision::Pass);
}

#[test]
fn a_single_and_a_double_tap_of_the_same_modifier_bound_together_each_fire_as_recognized() {
    // The guard in the launcher refuses a single and a double tap of the
    // same modifier bound together ("cannot coexist"); a record edited
    // by hand can hold both anyway, and the recognizer reports each as
    // it recognizes it: the release fires the tap, the second press the
    // double.
    let mut recognizer = Recognizer::default();
    recognizer.add(1, tap(WIN_AT, Side::Any));
    recognizer.add(2, double(WIN_AT, Side::Any));
    recognizer.step(at(LWIN, true, 0));
    assert_eq!(
        recognizer.step(at(LWIN, false, 100)),
        Decision::Tap {
            binding: 1,
            mask: true,
        }
    );
    assert_eq!(
        recognizer.step(at(LWIN, true, 300)),
        Decision::Tap {
            binding: 2,
            mask: false,
        }
    );
}

#[test]
fn a_chord_names_sides_and_only_its_named_modifiers() {
    let mut recognizer = Recognizer::default();
    // Replacing the Any of a modifier with a side in the binding:
    // Right Alt+G — and Right Alt+H, its own key, so the two keep apart.
    let mut sides = [None; 4];
    sides[ALT_AT] = Some(Side::Right);
    recognizer.add(
        1,
        Binding {
            kind: Kind::Chord,
            modifiers: sides,
            key: G,
            numpad: false,
        },
    );
    recognizer.add(
        2,
        Binding {
            kind: Kind::Chord,
            modifiers: sides,
            key: H,
            numpad: false,
        },
    );
    // The left Alt is not the right one: the chord does not fire.
    for decision in press(&mut recognizer, &[ALT], G) {
        assert_eq!(decision, Decision::Pass);
    }
    // The right Alt is.
    assert_eq!(
        press(&mut recognizer, &[0xA5], G), // VK_RMENU
        vec![
            Decision::Pass,
            Decision::Fire(1),
            Decision::Swallow,
            Decision::Pass,
        ]
    );
    // Right Alt+H fires the sided binding; left Alt+H does not.
    assert_eq!(recognizer.step(event(0xA5, true)), Decision::Pass);
    assert_eq!(recognizer.step(event(H, true)), Decision::Fire(2));
    recognizer.step(event(H, false));
    recognizer.step(event(0xA5, false));
    recognizer.step(event(ALT, true));
    assert_eq!(recognizer.step(event(H, true)), Decision::Pass);
}

#[test]
fn either_side_serves_a_modifier_no_side_is_named_for() {
    // The generic codes the tools that inject input send, and each
    // side's own code, all hold the modifier for a binding that names no
    // side.
    for control in [0x11, CONTROL, RCONTROL] {
        for alt in [0x12, ALT, 0xA5] {
            let mut recognizer = Recognizer::default();
            recognizer.add(1, chord([true, true, false, false], G));
            recognizer.step(event(control, true));
            recognizer.step(event(alt, true));
            assert_eq!(recognizer.step(event(G, true)), Decision::Fire(1));
        }
    }
    // A binding that names the left key is held by the left key alone or
    // by both; the right key alone does not serve it.
    let mut left = Recognizer::default();
    let mut sides = [None; 4];
    sides[CTRL_AT] = Some(Side::Left);
    left.add(
        1,
        Binding {
            kind: Kind::Chord,
            modifiers: sides,
            key: G,
            numpad: false,
        },
    );
    left.step(event(CONTROL, true));
    left.step(event(RCONTROL, true));
    // Both keys down: the left one is held, so the chord fires.
    assert_eq!(left.step(event(G, true)), Decision::Fire(1));
    left.step(event(G, false));
    left.step(event(CONTROL, false));
    left.step(event(RCONTROL, true));
    // Only the right key down now.
    assert_eq!(left.step(event(G, true)), Decision::Pass);
}

#[test]
fn the_windows_key_s_release_is_masked_after_a_chord_pane_swallowed() {
    let mut recognizer = Recognizer::default();
    let mut win = [None; 4];
    win[WIN_AT] = Some(Side::Any);
    recognizer.add(
        1,
        Binding {
            kind: Kind::Chord,
            modifiers: win,
            key: G,
            numpad: false,
        },
    );
    // The chord fires and its key is swallowed; the Windows key's
    // release, reaching Windows as a lone tap's, would open the Start
    // menu — so the recognizer asks for the mask (#260).
    assert_eq!(recognizer.step(event(LWIN, true)), Decision::Pass);
    assert_eq!(recognizer.step(event(G, true)), Decision::Fire(1));
    assert_eq!(recognizer.step(event(G, false)), Decision::Swallow);
    assert_eq!(recognizer.step(event(LWIN, false)), Decision::Mask);
    // A chord a tool injected is reported but not swallowed: Windows saw
    // its key, so the release opens no Start menu and is not masked.
    let injected = |key: u32, pressed: bool| KeyEvent {
        injected: true,
        ..event(key, pressed)
    };
    recognizer.step(injected(LWIN, true));
    assert_eq!(recognizer.step(injected(G, true)), Decision::Injected(1));
    recognizer.step(injected(G, false));
    assert_eq!(recognizer.step(injected(LWIN, false)), Decision::Pass);
    // A Windows key released after a key Pane did not take passes as it
    // is: Win+<key> keeps Windows' meaning.
    recognizer.step(event(LWIN, true));
    recognizer.step(event(E, true));
    recognizer.step(event(E, false));
    assert_eq!(recognizer.step(event(LWIN, false)), Decision::Pass);
}

#[test]
fn the_numpad_s_enter_is_told_from_the_main_one() {
    let mut recognizer = Recognizer::default();
    recognizer.add(1, chord([true, false, false, false], ENTER));
    recognizer.add(
        2,
        Binding {
            kind: Kind::Chord,
            modifiers: [Some(Side::Any), None, None, None],
            key: ENTER,
            numpad: true,
        },
    );
    // The main Enter, not extended: the first binding.
    recognizer.step(event(CONTROL, true));
    assert_eq!(recognizer.step(event(ENTER, true)), Decision::Fire(1));
    recognizer.step(event(ENTER, false));
    recognizer.step(event(CONTROL, false));
    // The numpad's Enter, extended: the second, distinct from its
    // counterpart (#260).
    recognizer.step(event(CONTROL, true));
    let numpad = KeyEvent {
        extended: true,
        ..event(ENTER, true)
    };
    assert_eq!(recognizer.step(numpad), Decision::Fire(2));
    let numpad_up = KeyEvent {
        extended: true,
        ..event(ENTER, false)
    };
    assert_eq!(recognizer.step(numpad_up), Decision::Swallow);
}

#[test]
fn a_recording_session_holds_the_keys_back_and_reports_what_was_pressed() {
    let mut recognizer = Recognizer::default();
    recognizer.recording(true);
    // A chord candidate: the key pressed with the modifiers held, each
    // any side or the one named. Every event is held back — the Start
    // menu the Windows key alone opens stays closed — and a candidate
    // the recorder would refuse is reported as it is, for the recorder's
    // own checks to refuse.
    assert_eq!(recognizer.step(event(CONTROL, true)), Decision::Swallow);
    assert_eq!(recognizer.step(event(ALT, true)), Decision::Swallow);
    assert_eq!(
        recognizer.step(event(G, true)),
        Decision::Recorded(Recorded::Chord {
            modifiers: [Some(Side::Left), Some(Side::Left), None, None],
            key: G,
            numpad: false,
        })
    );
    assert_eq!(recognizer.step(event(G, false)), Decision::Swallow);
    // The modifiers up: what follows is pressed alone.
    recognizer.step(event(CONTROL, false));
    recognizer.step(event(ALT, false));
    // A bare key, with no modifiers: reported for the recorder to
    // explain, as the window's own recorder does.
    assert_eq!(
        recognizer.step(event(H, true)),
        Decision::Recorded(Recorded::Chord {
            modifiers: [None, None, None, None],
            key: H,
            numpad: false,
        })
    );
    recognizer.step(event(H, false));

    // A lone tap of the Windows key, recognized by its timing, reported
    // and held back — no mask needed, the release never reaching
    // Windows.
    assert_eq!(recognizer.step(at(LWIN, true, 0)), Decision::Swallow);
    assert_eq!(
        recognizer.step(at(LWIN, false, 100)),
        Decision::Recorded(Recorded::Tap {
            modifier: WIN_AT,
            side: Side::Left,
        })
    );
    // A double tap, likewise: the first press and its release — a lone
    // tap in its own right — are held back, the release reporting the
    // tap, and the second press, with nothing between, the double.
    assert_eq!(recognizer.step(at(RCONTROL, true, 200)), Decision::Swallow);
    assert_eq!(
        recognizer.step(at(RCONTROL, false, 300)),
        Decision::Recorded(Recorded::Tap {
            modifier: CTRL_AT,
            side: Side::Right,
        })
    );
    assert_eq!(
        recognizer.step(at(RCONTROL, true, 500)),
        Decision::Recorded(Recorded::Double {
            modifier: CTRL_AT,
            side: Side::Right,
        })
    );
    recognizer.step(at(RCONTROL, false, 600));

    // An auto-repeat of a key held is held back and reports nothing.
    recognizer.step(event(G, true));
    assert_eq!(recognizer.step(event(G, true)), Decision::Swallow);

    // Pane's own tagged keys pass through untouched, even while
    // recording: Pane never reacts to its own input.
    let tagged = KeyEvent {
        tag: INJECTED_TAG,
        ..event(G, true)
    };
    assert_eq!(recognizer.step(tagged), Decision::Pass);

    // The session over: the keys reach the system again, and a chord
    // Pane had bound before it fires as it did.
    recognizer.recording(false);
    recognizer.add(1, chord([true, true, false, false], G));
    assert_eq!(
        press(&mut recognizer, &[CONTROL, ALT], G),
        vec![
            Decision::Pass,
            Decision::Pass,
            Decision::Fire(1),
            Decision::Swallow,
            Decision::Pass,
            Decision::Pass,
        ]
    );
}

#[test]
fn a_recording_session_reports_a_chord_the_tools_inject_too() {
    // The smoke's SendKeys and the remappers inject the keys the user
    // pressed; a session reports them, as the recognizer does for a
    // bound chord.
    let mut recognizer = Recognizer::default();
    recognizer.recording(true);
    let injected = |key: u32, pressed: bool| KeyEvent {
        injected: true,
        ..event(key, pressed)
    };
    assert_eq!(recognizer.step(injected(CONTROL, true)), Decision::Swallow);
    assert_eq!(
        recognizer.step(injected(G, true)),
        Decision::Recorded(Recorded::Chord {
            modifiers: [Some(Side::Left), None, None, None],
            key: G,
            numpad: false,
        })
    );
}
