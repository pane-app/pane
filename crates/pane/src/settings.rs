//! Pane's host settings: the appearance preferences, the Open Pane hotkey,
//! the launch-at-login choice, the tray visibility, and the Launcher
//! page's choices — the display the launcher opens on and what reopening
//! shows — held as one observable entity every window renders through.
//!
//! [`init`] reads the record — `settings.json` in Pane's data folder, kept
//! by [`pane_core::HostSettings`] under the house record rules — applies
//! the development overrides `PANE_THEME` and `PANE_MATERIAL`, and makes
//! the entity the app's global. [`shared`] hands it to whichever code
//! holds an [`App`], and [`visuals`] reads what every window renders with:
//! the theme and material the preferences resolve to, the system's
//! appearance included where the preference follows it.
//!
//! The entity is *observable*: both windows — the launcher's and
//! Settings' — register, in their constructors, an observer that repaints
//! the window and re-applies its background when the settings change, so
//! one choice updates both at once with no restart. The same observers
//! feed the system's appearance changes back into the entity, so a
//! "follow the system" theme re-renders when the operating system switches
//! between light and dark (GPUI delivers the platform's notification on
//! every supported platform; the test platform simulates it).
//!
//! Persistence follows the record's own discipline. A choice takes effect
//! in both windows immediately and is then written off the window's
//! thread, one write at a time, each holding the choices as they are when
//! it begins, so rapid changes converge on the last of them. A write that
//! fails is reported (the appearance page shows it as its status) and the
//! displayed choice goes back to what the record last held, so what the
//! page shows is what Pane actually saved — the saved preference and the
//! effective state stay distinguishable, and a fresh start reloads the
//! last successfully saved choice. The Open Pane hotkey follows the same
//! rule with one more step: it is registered with the system *before* it
//! is kept, and a write that fails also releases the registration the
//! record does not hold, so what the record names is what works. The
//! tray or menu-bar visibility follows the same step: the native entry
//! is shown or hidden *before* the choice is kept, so a change the
//! system refuses is explained and nothing is saved, and a write that
//! fails takes the entry back to what the record holds. A record
//! that cannot be read is never replaced: the choice is refused, the
//! reason is reported, and the source data stays on disk for diagnosis.
//!
//! The development overrides are a separate matter: they win for this
//! process — the record is not consulted for what the windows render —
//! but they are never written back as the user's preference, and the
//! appearance page says they are in force. [`ensure`] is the fallback for
//! windows built without a startup step (the tests'): an in-memory entity
//! with the product defaults and nowhere to save, and no environment
//! overrides, so those windows render deterministically.
//!
//! The launch-at-login choice is the same discipline with one more
//! party: the platform's own registration, reached through the
//! [`pane_core::autostart`] adapter the entity holds. The registration
//! is changed before the record is written, so a platform that refuses
//! leaves the preference, the registration and the record as they were;
//! at start the saved choice is reconciled with the registration the
//! platform reports (a missing or stale one repaired, a registration
//! the user did not choose removed, a disabled choice never silently
//! enabled); and the entity keeps the three truths apart — the saved
//! preference, the registration the platform reports, and the
//! integration's own unavailability — so the General page can show all
//! three instead of one pretense. The adapters the tests inject are
//! fakes; an entity built without one manages no registration at all.
//!
//! The launcher's background image (ADR 0028) is recorded as the Launcher
//! page's choices are, after one step of its own: the picture the user
//! chooses is copied into the data folder off the window's thread, and only
//! the kept copy's name is recorded (see [`crate::background`]); a copy the
//! record no longer names is removed once the record is written. The
//! entity also bakes the launcher's backdrop from the copy, off the
//! window's thread, as the launcher asks for it while drawing
//! ([`Settings::request_backdrop`]), and [`launcher_visuals`] hands the
//! launcher its visuals over it. The Settings window keeps the plain
//! [`visuals`].
//!
//! What this module does not do: it makes no palette or surface decision
//! of its own. [`crate::ui::Visuals`] stays presentation — this module
//! resolves the preferences into it, including the platform's
//! normalization of a glass request to the solid surface, and the page
//! explains that.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, Entity, Global, Window, WindowAppearance,
    WindowBackgroundAppearance,
};
use pane_core::autostart::{Autostart, Registration};
use pane_core::hotkeys::Shortcut;
use pane_core::tray::{Tray, TrayError};
use pane_core::{
    BackgroundEffect, Binding, HostSettings, Keyboard, KeyboardAction, Launcher,
    MaterialPreference, ThemePreference,
};

use crate::background::{self, Backdrop};
use crate::ui::Visuals;
use crate::ui::material::{Material, MaterialMode};
use crate::ui::theme::{Appearance, Theme};

/// One development override of the appearance, named in the environment:
/// `PANE_THEME` (`system`, `light` or `dark`) and `PANE_MATERIAL`
/// (`glass` or `opaque`). An override wins for this process, is visibly
/// indicated in Settings, and is never written back as the user's saved
/// preference.
///
/// Tests name their overrides directly through [`Self::parse`] instead of
/// the environment, which is process-global and shared with every test
/// running beside them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overrides {
    /// The theme this process renders with, whatever the record says.
    pub theme: Option<ThemePreference>,
    /// The material this process renders with, whatever the record says.
    pub material: Option<MaterialPreference>,
}

impl Overrides {
    /// Whether no override is in force: the record's preferences are
    /// what the windows render, and the page offers its choices.
    pub fn is_empty(&self) -> bool {
        self.theme.is_none() && self.material.is_none()
    }

    /// The overrides the environment names in `PANE_THEME` and
    /// `PANE_MATERIAL`. Unknown or missing values override nothing: the
    /// record's preference stands, as it does with no override at all.
    pub fn from_env() -> Overrides {
        Self::parse(
            std::env::var("PANE_THEME").ok().as_deref(),
            std::env::var("PANE_MATERIAL").ok().as_deref(),
        )
    }

    /// The overrides the strings `theme` and `material` name, as
    /// `PANE_THEME` and `PANE_MATERIAL` would hold them. Unknown or
    /// missing values override nothing.
    pub fn parse(theme: Option<&str>, material: Option<&str>) -> Overrides {
        let theme = match theme {
            Some("system") => Some(ThemePreference::System),
            Some("light") => Some(ThemePreference::Light),
            Some("dark") => Some(ThemePreference::Dark),
            _ => None,
        };
        let material = match material {
            Some("glass") => Some(MaterialPreference::Glass),
            Some("opaque") => Some(MaterialPreference::Solid),
            _ => None,
        };
        Overrides { theme, material }
    }

    /// The `NAME=value` pairs these overrides name, for the appearance
    /// page's notice.
    fn descriptions(&self) -> Vec<String> {
        let mut descriptions = Vec::new();
        if let Some(theme) = self.theme {
            descriptions.push(format!("PANE_THEME={}", word(theme)));
        }
        if let Some(material) = self.material {
            descriptions.push(format!("PANE_MATERIAL={}", material_word(material)));
        }
        descriptions
    }
}

/// The word the record (and the environment) writes `preference` as.
fn word(preference: ThemePreference) -> &'static str {
    match preference {
        ThemePreference::Dark => "dark",
        ThemePreference::Light => "light",
        ThemePreference::System => "system",
    }
}

/// The word the record (and the environment) writes a material
/// preference as — the solid surface is `opaque`, as the old startup
/// environment variable named it.
fn material_word(preference: MaterialPreference) -> &'static str {
    match preference {
        MaterialPreference::Glass => "glass",
        MaterialPreference::Solid => "opaque",
    }
}

/// The launch-at-login integration: the system adapter Pane reaches the
/// platform through, and the registration it last reported — the
/// *effective* state, as distinct from the user's saved preference and
/// from the platform's ability to manage a registration at all.
struct Login {
    /// The adapter: the platform's own, or one a test injects in its
    /// place. One adapter for the whole entity's life.
    adapter: Arc<dyn Autostart>,
    /// The registration the adapter last reported: its own query, or the
    /// outcome of the last change that succeeded; `Err` the problem the
    /// last query or change failed with, which the General page explains
    /// instead of showing a preference that pretends.
    state: Result<Registration, String>,
}

/// The host settings as one observable entity: what the record holds, what
/// Pane shows and saves now, the process's development overrides, the
/// system's appearance as the windows observe it, the effective
/// visuals every window renders with, and the launch-at-login
/// integration the General page manages. Built by [`init`] (or
/// [`ensure`]); the accessors below are what windows and the settings
/// pages read, and the setters what the pages drive.
pub(crate) struct Settings {
    /// Pane's data folder, where the record is kept; `None` when there is
    /// none, so choices last only until this Pane quits.
    dir: Option<PathBuf>,
    /// The preferences as they stand in Pane: what the page shows, what a
    /// change sets, what a save writes.
    chosen: HostSettings,
    /// The preferences as the record last held them: what a fresh start
    /// reloads and what a failed save falls back to.
    saved: HostSettings,
    /// Why the record could not be read, if it could not; it is then never
    /// replaced, so its data stays on disk for diagnosis.
    unreadable: Option<String>,
    /// The development overrides, which win over both records and choices
    /// for this process.
    overrides: Overrides,
    /// The system's appearance, as a window last observed it.
    system: Appearance,
    /// The last save attempt's failure, if any; the page reports it.
    save_error: Option<String>,
    /// Whether a save is in flight, so writes happen one at a time.
    saving: bool,
    /// Whether a choice arrived while a save was in flight, to save what
    /// is chosen then once the write answers.
    pending: bool,
    /// The launch-at-login integration, whose registration the General
    /// page manages through the entity (see [`Login`]).
    login: Login,
    /// What every window renders with, recomputed whenever any input to it
    /// changes.
    effective: Visuals,
    /// The launcher that applies the Open Pane hotkey to the system, once a
    /// launcher window has attached it (see [`attach_launcher`]); `None`
    /// until then, so the choice still persists without one to apply it.
    launcher: Option<Launcher>,
    /// The adapter that shows and hides the native tray or menu-bar
    /// entry, once one has been attached (see [`attach_tray`]); `None`
    /// until then, so the choice still persists with nothing to apply
    /// it to.
    tray: Option<Arc<dyn Tray>>,
    /// Why the native entry is not in the state the preference names, if
    /// it is not — a show or hide the system refused. The standing
    /// unavailability of a whole platform (Linux today) is read from the
    /// adapter itself, not kept here.
    tray_problem: Option<String>,
    /// The launcher's background image as last baked (ADR 0028): what it
    /// was baked for, and the backdrop or why it could not be made. The
    /// launcher keeps drawing a backdrop until the next one is ready.
    backdrop: Option<(background::Key, Result<Backdrop, String>)>,
    /// What the bake in flight is for, if one is.
    baking: Option<background::Key>,
    /// How many chosen pictures are being copied into the data folder;
    /// nothing is pruned from the backgrounds folder meanwhile, so a copy
    /// is never removed before its choice is recorded.
    importing: usize,
    /// Why the last picture chosen could not be kept, if it could not.
    import_problem: Option<String>,
}

impl Settings {
    /// The settings over the record in `dir`, with `overrides` in force,
    /// the system's appearance as the app reports it now, and the login
    /// `integration` the General page manages. An unreadable record loads
    /// the defaults beside its problem, which the page reports; it is
    /// never replaced while Pane runs. The saved launch-at-login choice
    /// is reconciled with the registration the integration reports.
    fn open(
        dir: Option<PathBuf>,
        overrides: Overrides,
        integration: Arc<dyn Autostart>,
        cx: &Context<Self>,
    ) -> Settings {
        let (saved, unreadable) = match &dir {
            Some(dir) => match HostSettings::open(dir) {
                Ok(saved) => (saved, None),
                Err(problem) => (HostSettings::default(), Some(problem)),
            },
            None => (HostSettings::default(), None),
        };
        let system = appearance_of(cx.window_appearance());
        let chosen = saved.clone();
        let mut settings = Settings {
            dir,
            effective: visuals_of(
                overrides.theme.unwrap_or(chosen.theme),
                overrides.material.unwrap_or(chosen.material),
                system,
            ),
            chosen,
            saved,
            unreadable,
            overrides,
            system,
            save_error: None,
            saving: false,
            pending: false,
            login: Login {
                adapter: integration,
                state: Ok(Registration::Disabled),
            },
            launcher: None,
            tray: None,
            tray_problem: None,
            backdrop: None,
            baking: None,
            importing: 0,
            import_problem: None,
        };
        settings.reconcile_login();
        settings
    }

    /// The theme the windows render by: the override's, or the chosen
    /// preference's.
    pub(crate) fn theme_preference(&self) -> ThemePreference {
        self.overrides.theme.unwrap_or(self.chosen.theme)
    }

    /// The material the windows render by: the override's, or the chosen
    /// preference's. This is the *preference*, not the effective surface:
    /// a glass preference on a platform without compositor frost still
    /// renders the solid surface, which the page says.
    pub(crate) fn material_preference(&self) -> MaterialPreference {
        self.overrides.material.unwrap_or(self.chosen.material)
    }

    /// The material actually in effect: the preference, normalized by the
    /// platform (`Material::new`'s rule — glass without frost becomes the
    /// solid surface). Used for the window background a new window asks
    /// for; the page shows the preference and explains the difference.
    pub(crate) fn material(&self) -> Material {
        self.effective.material
    }

    /// The `NAME=value` pairs the overrides in force name; empty when none
    /// is, in which case the page offers its choices normally.
    pub(crate) fn override_descriptions(&self) -> Vec<String> {
        self.overrides.descriptions()
    }

    /// What the appearance page reports as its status, if anything: the
    /// last save attempt's failure, or the reason choices cannot be saved
    /// at all (an unreadable record, or no data folder).
    pub(crate) fn status(&self) -> Option<String> {
        if let Some(error) = &self.save_error {
            return Some(error.clone());
        }
        if let Some(problem) = &self.unreadable {
            return Some(format!(
                "Pane could not read the settings record, so your choices are not saved: \
                 {problem}"
            ));
        }
        None
    }

    /// Chooses `preference` for the theme: both windows re-render with it
    /// at once, and the record is written off the window's thread. An
    /// override in force, or an unreadable record, refuses the choice —
    /// nothing visible would change, and the record's rule is not to
    /// replace what cannot be read.
    pub(crate) fn set_theme(&mut self, preference: ThemePreference, cx: &mut Context<Self>) {
        let mut chosen = self.chosen.clone();
        chosen.theme = preference;
        self.commit(chosen, Taken::Appearance, cx);
    }

    /// Chooses `preference` for the material, as [`Settings::set_theme`]
    /// does.
    pub(crate) fn set_material(&mut self, preference: MaterialPreference, cx: &mut Context<Self>) {
        let mut chosen = self.chosen.clone();
        chosen.material = preference;
        self.commit(chosen, Taken::Appearance, cx);
    }

    /// The display the launcher opens on: what the Launcher page shows and
    /// what a fresh start places the window by.
    pub(crate) fn opening_monitor(&self) -> pane_core::OpeningMonitor {
        self.chosen.opening_monitor
    }

    /// Chooses the display the launcher opens on: a preference the window
    /// layer resolves against the display layout each time the launcher
    /// opens, so nothing is applied here — the record is written as the
    /// appearance choices are, and a write that fails is reported with the
    /// shown choice rolled back. An unreadable record refuses the choice,
    /// as it refuses the appearance's.
    pub(crate) fn set_opening_monitor(
        &mut self,
        monitor: pane_core::OpeningMonitor,
        cx: &mut Context<Self>,
    ) {
        let mut chosen = self.chosen.clone();
        chosen.opening_monitor = monitor;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// What reopening the launcher shows: the view it was left on, when
    /// still valid, or root search.
    pub(crate) fn reopening(&self) -> pane_core::Reopening {
        self.chosen.reopening
    }

    /// Chooses what reopening the launcher shows, as
    /// [`Settings::set_opening_monitor`] records the opening display.
    pub(crate) fn set_reopening(
        &mut self,
        reopening: pane_core::Reopening,
        cx: &mut Context<Self>,
    ) {
        let mut chosen = self.chosen.clone();
        chosen.reopening = reopening;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// How much of the launcher shows while the query is blank.
    pub(crate) fn window_mode(&self) -> pane_core::WindowMode {
        self.chosen.window_mode
    }

    /// Chooses the window mode; the launcher window resizes as it next
    /// draws.
    pub(crate) fn set_window_mode(&mut self, mode: pane_core::WindowMode, cx: &mut Context<Self>) {
        let mut chosen = self.chosen.clone();
        chosen.window_mode = mode;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// Whether the compact window shows the pins as a row of icons under
    /// the search field.
    pub(crate) fn compact_pinned(&self) -> bool {
        self.chosen.compact_pinned
    }

    /// Chooses whether the compact window shows the pins; the launcher
    /// window resizes as it next draws.
    pub(crate) fn set_compact_pinned(&mut self, on: bool, cx: &mut Context<Self>) {
        let mut chosen = self.chosen.clone();
        chosen.compact_pinned = on;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// How the pinned home lays out its quick slots.
    pub(crate) fn pinned_layout(&self) -> pane_core::PinnedLayout {
        self.chosen.pinned_layout
    }

    /// Chooses the pinned home's layout.
    pub(crate) fn set_pinned_layout(
        &mut self,
        layout: pane_core::PinnedLayout,
        cx: &mut Context<Self>,
    ) {
        let mut chosen = self.chosen.clone();
        chosen.pinned_layout = layout;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// How strict root search's matching is; the launcher's next keystroke
    /// applies it.
    pub(crate) fn search_sensitivity(&self) -> pane_core::SearchSensitivity {
        self.chosen.search_sensitivity
    }

    /// Chooses how strict root search's matching is; the choice takes
    /// effect as the launcher applies it, on the next keystroke.
    pub(crate) fn set_search_sensitivity(
        &mut self,
        sensitivity: pane_core::SearchSensitivity,
        cx: &mut Context<Self>,
    ) {
        let mut chosen = self.chosen.clone();
        chosen.search_sensitivity = sensitivity;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// Whether root search learns from what the user chooses; turned off,
    /// nothing is recorded and ranking acts as if nothing was learned
    /// until it is reset. The same switch will also stop search history
    /// (#206).
    pub(crate) fn learning(&self) -> bool {
        self.chosen.learning
    }

    /// Chooses whether root search learns from what the user chooses; the
    /// launcher applies the choice at once, re-ranking the list it is
    /// showing.
    pub(crate) fn set_learning(&mut self, on: bool, cx: &mut Context<Self>) {
        let mut chosen = self.chosen.clone();
        chosen.learning = on;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// What the launcher's back key does.
    pub(crate) fn escape(&self) -> pane_core::EscapeBehavior {
        self.chosen.escape
    }

    /// Chooses what the launcher's back key does.
    pub(crate) fn set_escape(&mut self, escape: pane_core::EscapeBehavior, cx: &mut Context<Self>) {
        let mut chosen = self.chosen.clone();
        chosen.escape = escape;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// Whether Escape closes the Settings window.
    pub(crate) fn escape_closes_settings(&self) -> bool {
        self.chosen.escape_closes_settings
    }

    /// Chooses whether Escape closes the Settings window.
    pub(crate) fn set_escape_closes_settings(&mut self, closes: bool, cx: &mut Context<Self>) {
        let mut chosen = self.chosen.clone();
        chosen.escape_closes_settings = closes;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// The extra selection keys.
    pub(crate) fn navigation(&self) -> pane_core::NavigationBindings {
        self.chosen.navigation
    }

    /// Why `navigation` cannot be chosen, if it cannot: one of its keys is
    /// already an action's binding.
    pub(crate) fn navigation_conflict(
        &self,
        navigation: pane_core::NavigationBindings,
    ) -> Option<String> {
        let (previous, next) = navigation.bindings()?;
        KeyboardAction::ALL.into_iter().find_map(|action| {
            let bound = self.chosen.keyboard.binding(action);
            let id = bound.id();
            (id == previous || id == next).then(|| format!("{bound} is {}", action.title()))
        })
    }

    /// Chooses the extra selection keys: the keymap is re-made at once,
    /// then the record is written. A set whose keys an action already has
    /// is refused.
    pub(crate) fn set_navigation(
        &mut self,
        navigation: pane_core::NavigationBindings,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(conflict) = self.navigation_conflict(navigation) {
            return Err(conflict);
        }
        if self.unreadable.is_none() && self.chosen.navigation != navigation {
            crate::keyboard::rebuild(cx, &self.chosen.keyboard, navigation);
        }
        let mut chosen = self.chosen.clone();
        chosen.navigation = navigation;
        self.commit(chosen, Taken::Recorded, cx);
        Ok(())
    }

    /// The launcher's background image: the file name of Pane's copy of
    /// it in the data folder's backgrounds folder, or `None` for the plain
    /// panel.
    pub(crate) fn background(&self) -> Option<&str> {
        self.chosen.background.as_deref()
    }

    /// The texture drawn into the background image.
    pub(crate) fn background_effect(&self) -> BackgroundEffect {
        self.chosen.background_effect
    }

    /// Whether a chosen picture is still being copied into the data
    /// folder.
    pub(crate) fn importing_background(&self) -> bool {
        self.importing > 0
    }

    /// Why the background image is not drawn as chosen, if it is not: the
    /// last picture chosen could not be kept, or the kept copy could not
    /// be drawn. The Appearance section shows it.
    pub(crate) fn background_problem(&self) -> Option<String> {
        if let Some(problem) = &self.import_problem {
            return Some(problem.clone());
        }
        match &self.backdrop {
            Some((key, Err(problem))) if self.background() == Some(key.name.as_str()) => Some(
                format!("Pane could not draw the background image: {problem}"),
            ),
            _ => None,
        }
    }

    /// Chooses the picture at `source` as the launcher's background image:
    /// Pane's own copy of it is made off the window's thread (see
    /// [`crate::background::import`]), and the choice is recorded once the
    /// copy is kept, as the Launcher page's choices are. A file that is
    /// not a picture, or a copy that cannot be kept, is reported and
    /// changes nothing. With an unreadable record, or no data folder to
    /// keep the copy in, the choice is refused at once.
    pub(crate) fn choose_background(&mut self, source: PathBuf, cx: &mut Context<Self>) {
        if self.refuse_unreadable(cx) {
            return;
        }
        let Some(dir) = self.dir.clone() else {
            self.import_problem =
                Some("Pane has no data folder to keep a background image in".into());
            cx.notify();
            return;
        };
        self.importing += 1;
        self.import_problem = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let imported = cx
                .background_executor()
                .spawn(async move { background::import(&source, &dir) })
                .await;
            this.update(cx, |settings, cx| {
                settings.importing -= 1;
                match imported {
                    Ok(name) => {
                        let mut chosen = settings.chosen.clone();
                        chosen.background = Some(name);
                        settings.commit(chosen, Taken::Recorded, cx);
                    }
                    Err(problem) => settings.import_problem = Some(problem),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Reports why no picture could be chosen — the platform's file picker
    /// could not open — where an import's problem shows.
    pub(crate) fn background_refused(&mut self, problem: String, cx: &mut Context<Self>) {
        self.import_problem = Some(problem);
        cx.notify();
    }

    /// Removes the launcher's background image: the plain panel comes back
    /// at once, and Pane's copy of the picture is removed once the record
    /// no longer names it.
    pub(crate) fn clear_background(&mut self, cx: &mut Context<Self>) {
        self.import_problem = None;
        let mut chosen = self.chosen.clone();
        chosen.background = None;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// Chooses the texture drawn into the background image; the backdrop
    /// is baked again as the launcher next draws.
    pub(crate) fn set_background_effect(
        &mut self,
        effect: BackgroundEffect,
        cx: &mut Context<Self>,
    ) {
        let mut chosen = self.chosen.clone();
        chosen.background_effect = effect;
        self.commit(chosen, Taken::Recorded, cx);
    }

    /// Asks for the backdrop the launcher window draws at `scale`, its
    /// scale factor, as it draws: a backdrop is baked off the window's
    /// thread only when the picture, its effect, the palette, the scale or
    /// the material changed since the last bake (or the one in flight), and the
    /// launcher keeps drawing the last one until it is ready, then
    /// repaints. With no background image chosen there is no backdrop.
    /// Nothing here repaints by itself: the launcher calls it while it
    /// draws.
    pub(crate) fn request_backdrop(&mut self, scale: f32, cx: &mut Context<Self>) {
        let Some((dir, name)) = self.dir.clone().zip(self.chosen.background.clone()) else {
            self.backdrop = None;
            self.baking = None;
            return;
        };
        let key = background::Key {
            name,
            effect: self.chosen.background_effect,
            appearance: resolve(self.theme_preference(), self.system),
            scale,
            glass: self.material().is_glass(),
        };
        let baked = self.backdrop.as_ref().map(|(baked, _)| baked);
        if baked == Some(&key) || self.baking.as_ref() == Some(&key) {
            return;
        }
        self.baking = Some(key.clone());
        let file = background::path(&dir, &key.name);
        cx.spawn(async move |this, cx| {
            let baking = key.clone();
            let baked = cx
                .background_executor()
                .spawn(async move { background::bake(&file, &baking) })
                .await;
            this.update(cx, |settings, cx| {
                // A bake another request has overtaken is dropped; the
                // newer one is in flight.
                if settings.baking.as_ref() == Some(&key) {
                    settings.baking = None;
                    settings.backdrop = Some((key, baked));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The backdrop the launcher draws, if a background image is chosen
    /// and one is ready.
    fn backdrop(&self) -> Option<&Backdrop> {
        self.chosen.background.as_ref()?;
        match &self.backdrop {
            Some((_, Ok(backdrop))) => Some(backdrop),
            _ => None,
        }
    }

    /// The Open Pane hotkey the host settings hold: what the General page
    /// shows and what a fresh start registers.
    pub(crate) fn open_pane(&self) -> Shortcut {
        self.chosen.open_pane.clone()
    }

    /// The in-app navigation bindings the host settings hold: what the
    /// Keyboard page shows and what every window's keys follow.
    pub(crate) fn keyboard(&self) -> Keyboard {
        self.chosen.keyboard.clone()
    }

    /// Records `binding` for `action`, one of the bounded set of in-app
    /// navigation actions: it is applied to every window's keys at once
    /// (the keymap is re-made over the new set, so the binding it replaces
    /// stops working) and only then written to the record off the window's
    /// thread. A binding that is protected for a focused field, or one
    /// another action of the set already has, is refused — `Err` names the
    /// problem and nothing changes. A write that fails rolls the choice
    /// back to what the record holds and re-applies it, so the keys that
    /// work are the keys the record names (see [`Settings::written`]).
    pub(crate) fn set_keyboard(
        &mut self,
        action: KeyboardAction,
        binding: Binding,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        // The record's rule: never replace what cannot be read.
        if let Some(refusal) = self.unreadable_refusal(Refusal::Change("shortcut")) {
            return Err(refusal);
        }
        // The extra selection keys are taken too.
        if let Some((previous, next)) = self.chosen.navigation.bindings()
            && (binding.id() == previous || binding.id() == next)
        {
            return Err(format!(
                "{binding} already moves the selection: turn off the navigation bindings first"
            ));
        }
        let mut chosen = self.chosen.clone();
        chosen.keyboard.checked_set(action, binding)?;
        if chosen == self.chosen {
            // The binding it already has: a retry that made it apply needs
            // no new record.
            cx.notify();
            return Ok(());
        }
        crate::keyboard::rebuild(cx, &chosen.keyboard, chosen.navigation);
        self.commit(chosen, Taken::Recorded, cx);
        Ok(())
    }

    /// Whether the tray or menu-bar entry is shown: what the General
    /// page's toggle shows and what a fresh start shows.
    pub(crate) fn tray_visible(&self) -> bool {
        self.chosen.tray_visible
    }

    /// Why the platform has no tray or menu-bar entry at all, if it has
    /// none: the adapter's own explanation, which the page shows instead
    /// of offering a switch that would pretend. The one input the
    /// switch's offered state reads.
    pub(crate) fn tray_unavailable(&self) -> Option<String> {
        self.tray.as_ref().and_then(|tray| tray.unavailable())
    }

    /// Why the native tray or menu-bar entry is not in the state the
    /// preference names, if it is not: the platform's lack of an entry,
    /// or the reason the entry's state last parted from the record — a
    /// startup application or a rollback the system refused (a refused
    /// *change* is the page's own refusal note, as a refused recording
    /// is). What the page shows, so the preference and the native state
    /// stay distinguishable.
    pub(crate) fn tray_status(&self) -> Option<String> {
        self.tray_unavailable()
            .or_else(|| self.tray_problem.clone())
    }

    /// Chooses `visible` for the tray or menu-bar entry. It is applied
    /// through the attached adapter *first*, and only then kept as the
    /// choice and written to the record off the window's thread — so a
    /// change the system refuses is explained (by the page) and nothing
    /// is kept or saved, and a write that fails rolls the native entry
    /// back to what the record holds (see [`Settings::written`]). An
    /// unreadable record refuses the change, as it refuses every choice.
    /// `Err` names why the change was refused.
    pub(crate) fn set_tray_visible(
        &mut self,
        visible: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(tray) = self.tray.clone() else {
            return Err("Pane has no tray or menu-bar entry to configure".into());
        };
        // The record's rule: never replace what cannot be read.
        if let Some(refusal) = self.unreadable_refusal(Refusal::Change("entry")) {
            return Err(refusal);
        }
        if let Err(error) = tray.set_visible(visible) {
            // A refused change is the page's rejection, reported by the
            // caller — not the entry's standing state, which a retry may
            // still fix. The adapter is told the preference again, so the
            // visibility it keeps for later (the Windows icon added again
            // when Explorer restarts) is the preference's, not the refused
            // change's; the entry is already in that state, so nothing
            // else changes.
            if matches!(error, TrayError::Refused(_)) {
                let _ = tray.set_visible(self.chosen.tray_visible);
            }
            return Err(error.to_string());
        }
        // The change took: the entry's state matches the choice now.
        self.tray_problem = None;
        if self.chosen.tray_visible != visible {
            let mut chosen = self.chosen.clone();
            chosen.tray_visible = visible;
            self.commit(chosen, Taken::Recorded, cx);
        } else {
            // The record already holds it: a retry that made it take
            // effect needs no new record.
            cx.notify();
        }
        Ok(())
    }

    /// Hides the native entry without touching the choice or the record:
    /// the quit path, which removes Pane's own resources but changes no
    /// preference. A failure is not reported — there is no window left
    /// to report it to, and the system reclaims the entry with the
    /// process anyway.
    pub(crate) fn release_tray(&mut self) {
        if let Some(tray) = &self.tray {
            let _ = tray.set_visible(false);
        }
    }
    /// Records `shortcut` as the Open Pane hotkey, the application-owned
    /// binding that summons the launcher from any application. It is
    /// applied through the attached launcher *first* — checked against the
    /// combinations the system keeps for itself and the command hotkeys,
    /// and registered before the binding it replaces is released, so a
    /// refusal leaves the previous binding working — and only then kept
    /// as the choice and written to the record off the window's thread. A
    /// write that fails rolls the registration back to what the record
    /// holds (see [`Settings::written`]). `Err` names why the change was
    /// refused; nothing is kept or saved then.
    pub(crate) fn set_open_pane(
        &mut self,
        shortcut: Shortcut,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(launcher) = self.launcher.clone() else {
            return Err("Pane has no launcher to register the hotkey with".into());
        };
        // The record's rule: never replace what cannot be read.
        if let Some(refusal) = self.unreadable_refusal(Refusal::Change("hotkey")) {
            return Err(refusal);
        }
        launcher.set_open_pane(shortcut.clone())?;
        if self.chosen.open_pane != shortcut {
            let mut chosen = self.chosen.clone();
            chosen.open_pane = shortcut;
            self.commit(chosen, Taken::Recorded, cx);
        } else {
            // The record already holds it: a retry that made it register
            // needs no new record.
            cx.notify();
        }
        Ok(())
    }

    /// Whether the user chose Pane to start at login: the *saved
    /// preference*, as distinct from the registration the platform
    /// actually holds (see [`Settings::login_registration`]).
    pub(crate) fn launch_at_login(&self) -> bool {
        self.chosen.launch_at_login
    }

    /// Why the launch-at-login registration cannot be managed here at
    /// all, if it cannot — an unsupported platform, a development build,
    /// an operating system without the API. The General page explains
    /// the reason and offers no toggle; nothing pretends to manage a
    /// registration the platform will not let Pane touch.
    pub(crate) fn login_unavailable(&self) -> Option<String> {
        self.login.adapter.unavailable()
    }

    /// The registration the platform last reported: the *effective*
    /// state, as distinct from the preference. `Err` holds the problem
    /// the last query or change failed with, which the page explains.
    pub(crate) fn login_registration(&self) -> &Result<Registration, String> {
        &self.login.state
    }

    /// Chooses whether Pane starts at login: the platform's registration
    /// is changed first, so a refusal leaves the preference, the
    /// registration and the record as they were — a failed registration
    /// or removal never masquerades as a successful toggle — and the
    /// record is then written off the window's thread, as the appearance
    /// choices are. An unreadable record refuses the choice, as it
    /// refuses the appearance's: the record's rule is not to replace
    /// what cannot be read.
    pub(crate) fn set_launch_at_login(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled == self.chosen.launch_at_login {
            return;
        }
        if self.refuse_unreadable(cx) {
            return;
        }
        let changed = if enabled {
            self.login.adapter.enable()
        } else {
            self.login.adapter.disable()
        };
        match changed {
            Ok(registration) => {
                self.login.state = Ok(registration);
                let mut chosen = self.chosen.clone();
                chosen.launch_at_login = enabled;
                self.commit(chosen, Taken::Repainted, cx);
            }
            Err(problem) => {
                // An adapter that cannot manage the registration here
                // answers with its reason, which is exactly what the
                // page needs to show; the preference stays what it was.
                self.login.state = Err(problem);
                cx.notify();
            }
        }
    }

    /// Reconciles the saved launch-at-login choice with the registration
    /// the integration reports at start: a registration missing or stale
    /// (the platform's own state, not the record's) is repaired to what
    /// the user chose, and a registration the user did not choose is
    /// removed — a disabled choice is never silently enabled, because
    /// the repair only ever follows the record. A record that cannot be
    /// read, an integration that cannot manage the registration here, or
    /// a system that cannot be asked leaves the registration exactly as
    /// it is: the state is reported and the page explains the problem
    /// rather than guessing.
    fn reconcile_login(&mut self) {
        if self.unreadable.is_some() {
            // The record cannot be asked what the user chose; touching
            // the registration would impose a choice nobody made.
            return;
        }
        if let Some(reason) = self.login.adapter.unavailable() {
            self.login.state = Err(reason);
            return;
        }
        match self.login.adapter.registered() {
            Err(problem) => self.login.state = Err(problem),
            Ok(registration) if registration.registered() == self.chosen.launch_at_login => {
                self.login.state = Ok(registration);
            }
            Ok(_) => {
                let repair = if self.chosen.launch_at_login {
                    self.login.adapter.enable()
                } else {
                    self.login.adapter.disable()
                };
                self.login.state = repair;
            }
        }
    }

    /// Takes `chosen`, shows it as `taken` says, and saves. The single path
    /// every choice goes through: what it applies to the system first (a
    /// registration, the tray entry, the keymap) is the setter's, and a
    /// write that fails takes those back (see [`Settings::written`]). A
    /// choice that changes nothing is no choice; an appearance choice under
    /// a development override is refused silently, since nothing visible
    /// would change; and an unreadable record refuses every choice, which
    /// the page reports.
    fn commit(&mut self, chosen: HostSettings, taken: Taken, cx: &mut Context<Self>) {
        if chosen == self.chosen || (taken == Taken::Appearance && !self.overrides.is_empty()) {
            return;
        }
        if self.refuse_unreadable(cx) {
            return;
        }
        self.chosen = chosen;
        match taken {
            Taken::Appearance | Taken::Repainted => self.changed(cx),
            Taken::Recorded => cx.notify(),
        }
        self.save(cx);
    }

    /// Why a change is refused because the record cannot be read, in the
    /// words `refusal` says it with; `None` while the record is readable.
    /// The record's rule: never replace what cannot be read.
    fn unreadable_refusal(&self, refusal: Refusal) -> Option<String> {
        let problem = self.unreadable.as_ref()?;
        Some(match refusal {
            Refusal::Choice => format!("Pane does not replace it: {problem}"),
            Refusal::Change(what) => format!(
                "Pane could not read the settings record, so the {what} is not changed: {problem}"
            ),
        })
    }

    /// Refuses a choice because the record cannot be read, as the page's
    /// status reports it. Whether it was refused.
    fn refuse_unreadable(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(refusal) = self.unreadable_refusal(Refusal::Choice) else {
            return false;
        };
        self.save_error = Some(refusal);
        cx.notify();
        true
    }

    /// Records the appearance the system has now, as a window observed it
    /// (the windows register the platform's notification; see the module
    /// docs), and repaints where the theme follows the system.
    pub(crate) fn system_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        if self.system != appearance {
            self.system = appearance;
            self.changed(cx);
        }
    }

    /// Recomputes the effective visuals, tells the native window chrome to
    /// follow the theme (macOS's edges and titlebar; a no-op elsewhere),
    /// and notifies the windows observing these settings.
    fn changed(&mut self, cx: &mut Context<Self>) {
        let theme = self.theme_preference();
        let material = self.material_preference();
        self.effective = visuals_of(theme, material, self.system);
        cx.set_window_appearance(native_chrome(theme));
        cx.notify();
    }

    /// Writes the chosen preferences to the record, off the window's
    /// thread. One write is in flight at a time; a choice that arrives
    /// meanwhile is held and written, as it stands then, when the write
    /// answers, so rapid changes converge on the last of them.
    fn save(&mut self, cx: &mut Context<Self>) {
        if self.saving {
            self.pending = true;
            return;
        }
        let Some(dir) = self.dir.clone() else {
            self.save_error = Some(
                "Pane has no data folder to keep settings in, so this choice lasts only until \
                 Pane quits"
                    .into(),
            );
            cx.notify();
            return;
        };
        self.saving = true;
        self.save_error = None;
        let snapshot = self.chosen.clone();
        let writing = snapshot.clone();
        cx.spawn(async move |this, cx| {
            let written = cx
                .background_executor()
                .spawn(async move { writing.save(&dir) })
                .await;
            this.update(cx, |settings, cx| settings.written(written, snapshot, cx))
                .ok();
        })
        .detach();
    }

    /// Records what a write reported. On success the record now holds the
    /// snapshot. On failure the reason is reported, and the displayed
    /// choice goes back to what the record last held — unless the user
    /// chose something newer meanwhile, which the pending write (or the
    /// next choice) carries — so the page shows what Pane actually saved.
    fn written(
        &mut self,
        written: Result<(), String>,
        snapshot: HostSettings,
        cx: &mut Context<Self>,
    ) {
        self.saving = false;
        match written {
            Ok(()) => {
                let replaced = self.saved.background != snapshot.background;
                self.saved = snapshot;
                // A background image the record no longer names is
                // removed from the data folder — unless a picture is being
                // copied in, whose copy no record names yet. The folder
                // holds a copy or two, so this is a listing and a removal,
                // done here so no import can start in between.
                if replaced
                    && self.importing == 0
                    && let Some(dir) = &self.dir
                {
                    let keep: Vec<&str> = [&self.saved.background, &self.chosen.background]
                        .into_iter()
                        .flatten()
                        .map(String::as_str)
                        .collect();
                    background::prune(dir, &keep);
                }
            }
            Err(why) => {
                let mut problem = format!("Pane could not save your choice: {why}");
                if self.chosen == snapshot {
                    // The shown choice goes back to what the record holds,
                    // and so does every effect the failed choice applied to
                    // the system, so what works is what the record names.
                    // The windows repaint with the record's choice before
                    // the effects that follow the repaint are restored.
                    let applied: Vec<Effect> = Effect::ALL
                        .into_iter()
                        .filter(|effect| effect.differs(&snapshot, &self.saved))
                        .collect();
                    self.chosen = self.saved.clone();
                    let (before, after): (Vec<Effect>, Vec<Effect>) = applied
                        .into_iter()
                        .partition(|effect| !effect.follows_repaint());
                    self.restore(before, &mut problem, cx);
                    self.changed(cx);
                    self.restore(after, &mut problem, cx);
                }
                self.save_error = Some(problem);
            }
        }
        if self.pending {
            self.pending = false;
            self.save(cx);
        }
        cx.notify();
    }

    /// Takes each of `effects` back to what the record holds, after a
    /// write that failed (see [`Settings::written`]); the choices are the
    /// record's already. An undo that itself fails is appended to
    /// `problem`, reported beside the save's problem rather than hidden.
    fn restore(
        &mut self,
        effects: impl IntoIterator<Item = Effect>,
        problem: &mut String,
        cx: &mut Context<Self>,
    ) {
        for effect in effects {
            match effect {
                // The Open Pane registration follows the record back, so
                // a choice that could not be saved does not leave the
                // launcher bound to what the record does not hold; what
                // was last recorded keeps working.
                Effect::OpenPane => {
                    if let Some(launcher) = &self.launcher {
                        // The outcome is the binding's own state (the
                        // problem the page explains), not a value to
                        // surface here.
                        let _ = launcher.sync_open_pane(self.saved.open_pane.clone());
                    }
                }
                // The tray entry follows the record back too, by the
                // same rule: what the record last held is what the
                // native state goes back to, so the preference the page
                // shows matches what Pane actually saved.
                Effect::Tray => {
                    if let Some(tray) = &self.tray {
                        self.tray_problem = tray
                            .set_visible(self.saved.tray_visible)
                            .err()
                            .map(|error| error.to_string());
                    }
                }
                // The registration was changed for a choice that could not
                // be kept: undo it, so what Pane actually starts at login
                // is what the record last held — a failed save never
                // masquerades as a successful toggle.
                Effect::Login => {
                    let undone = if self.saved.launch_at_login {
                        self.login.adapter.enable()
                    } else {
                        self.login.adapter.disable()
                    };
                    match undone {
                        Ok(registration) => self.login.state = Ok(registration),
                        Err(undo) => {
                            *problem = format!(
                                "{problem}; Pane could not undo the login registration: {undo}"
                            )
                        }
                    }
                }
                // The in-app navigation bindings follow the record back
                // the same way: the keymap is re-made over what the
                // record holds, so a binding that could not be saved
                // stops working and the recorded one works again.
                Effect::Keymap => {
                    let keyboard = self.chosen.keyboard.clone();
                    crate::keyboard::rebuild(cx, &keyboard, self.chosen.navigation);
                }
            }
        }
    }
}

/// How a choice is taken through [`Settings::commit`], beyond keeping it
/// and writing the record: the one thing the choices differ in there.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Taken {
    /// An appearance choice, the theme or the material: a development
    /// override in force refuses it, and both windows repaint with it,
    /// the native chrome following the theme.
    Appearance,
    /// The launch-at-login choice: no override speaks for it, but taking
    /// it repaints as an appearance choice does.
    Repainted,
    /// Every other choice: the windows read it as they next draw, so they
    /// are only told to redraw. An override in force does not refuse
    /// these: the development overrides speak for the appearance only.
    Recorded,
}

/// The words a change is refused with while the record cannot be read
/// (see [`Settings::unreadable_refusal`]).
#[derive(Clone, Copy)]
enum Refusal {
    /// A choice the page's status reports the refusal of.
    Choice,
    /// A change its setter answers `Err` for, naming what is not changed
    /// ("shortcut", "entry", "hotkey"), which the page shows beside it.
    Change(&'static str),
}

/// A side effect a choice applies to the system before the record is
/// written — a registration, the tray entry, the keymap — which a write
/// that fails takes back to what the record holds (see
/// [`Settings::written`]). A new effect is added here: its variant, which
/// choices it follows, and how [`Settings::restore`] takes it back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Effect {
    /// The Open Pane hotkey's registration with the system.
    OpenPane,
    /// The native tray or menu-bar entry's visibility.
    Tray,
    /// The platform's launch-at-login registration.
    Login,
    /// Every window's key bindings: the in-app bindings and the extra
    /// navigation keys.
    Keymap,
}

impl Effect {
    /// Every effect, in the order a failed write restores them.
    const ALL: [Effect; 4] = [
        Effect::OpenPane,
        Effect::Tray,
        Effect::Login,
        Effect::Keymap,
    ];

    /// Whether `chosen` applied this effect differently from `saved`, so a
    /// failed write of `chosen` has it to take back.
    fn differs(self, chosen: &HostSettings, saved: &HostSettings) -> bool {
        match self {
            Effect::OpenPane => chosen.open_pane != saved.open_pane,
            Effect::Tray => chosen.tray_visible != saved.tray_visible,
            Effect::Login => chosen.launch_at_login != saved.launch_at_login,
            Effect::Keymap => {
                chosen.keyboard != saved.keyboard || chosen.navigation != saved.navigation
            }
        }
    }

    /// Whether it is restored after the windows repaint with the record's
    /// choices: the keymap is re-made last.
    fn follows_repaint(self) -> bool {
        self == Effect::Keymap
    }
}

/// The visuals `theme` and `material` resolve to with the system's
/// `appearance`: the palette for the preference (or the system's, where
/// the preference follows it), and the material normalized by the
/// platform — a glass request without compositor frost becomes the solid
/// surface, which [`crate::ui::material`] explains.
fn visuals_of(theme: ThemePreference, material: MaterialPreference, system: Appearance) -> Visuals {
    let mode = match material {
        MaterialPreference::Glass => MaterialMode::Glass,
        MaterialPreference::Solid => MaterialMode::Opaque,
    };
    Visuals {
        theme: Theme::new(resolve(theme, system)),
        material: Material::new(mode),
        backdrop: None,
    }
}

/// The palette the theme preference `theme` resolves to with the system's
/// `appearance`.
fn resolve(theme: ThemePreference, system: Appearance) -> Appearance {
    match theme {
        ThemePreference::Dark => Appearance::Dark,
        ThemePreference::Light => Appearance::Light,
        ThemePreference::System => system,
    }
}

/// The appearance a window appearance resolves to: the two light variants
/// and the two dark ones (macOS's vibrancy) to Pane's two palettes.
pub(crate) fn appearance_of(appearance: WindowAppearance) -> Appearance {
    match appearance {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}

/// The native window chrome to ask for, so the platform's own titlebar and
/// edges match the theme: forced light or dark where the preference forces
/// one, and `None` — follow the system — where the preference does.
fn native_chrome(theme: ThemePreference) -> Option<WindowAppearance> {
    match theme {
        ThemePreference::Light => Some(WindowAppearance::Light),
        ThemePreference::Dark => Some(WindowAppearance::Dark),
        ThemePreference::System => None,
    }
}

/// The one host-settings entity, as the app's global. The first
/// initialization makes it; every later lookup goes through it.
struct Shared(Entity<Settings>);

impl Global for Shared {}

/// Initializes the host settings and makes them the app's global: the
/// record in `dir` (Pane's data folder), with the development overrides
/// the environment names in force for this process, and the platform's
/// own login integration, whose registration is reconciled with the
/// saved launch-at-login choice. The first initializer wins — later
/// calls leave the existing settings alone. Call once, at startup,
/// before the first window opens.
pub fn init(dir: Option<PathBuf>, cx: &mut App) {
    init_with_login(
        dir,
        Overrides::from_env(),
        pane_core::autostart::native(),
        cx,
    );
}

/// [`init`] with the overrides named outright, so a caller (the tests)
/// fixes what the process overrides without touching the environment.
/// The login integration stays unset: an entity built this way manages
/// no registration — the tests inject their own adapter through
/// [`init_with_login`] — so no test ever touches the login configuration
/// of the machine it runs on.
pub fn init_with_overrides(dir: Option<PathBuf>, overrides: Overrides, cx: &mut App) {
    init_with_login(dir, overrides, pane_core::autostart::none(), cx);
}

/// [`init`] with the overrides *and* the login integration named
/// outright: the integration the entity manages the registration
/// through, which the binary leaves to the platform's own
/// ([`pane_core::autostart::native`]) and the tests replace with their
/// fake (see the General page's tests). The saved launch-at-login
/// choice is reconciled with the registration it reports, as at start.
pub fn init_with_login(
    dir: Option<PathBuf>,
    overrides: Overrides,
    integration: Arc<dyn Autostart>,
    cx: &mut App,
) {
    if cx.try_global::<Shared>().is_some() {
        return;
    }
    let settings = cx.new(|cx| Settings::open(dir, overrides, integration, cx));
    let theme = settings.read(cx).theme_preference();
    cx.set_global(Shared(settings));
    // The native chrome follows the theme the settings resolve to, so the
    // platform's own titlebar and window edges match what the windows
    // paint from the first frame.
    cx.set_window_appearance(native_chrome(theme));
}

/// The host settings, ensuring they exist: the global [`init`] made, or —
/// for a window built without a startup step, as the tests are — an
/// in-memory entity over the product defaults, with nowhere to save and
/// no environment overrides, so those windows render deterministically.
pub(crate) fn ensure(cx: &mut App) -> Entity<Settings> {
    if let Some(shared) = cx.try_global::<Shared>() {
        return shared.0.clone();
    }
    init_with_overrides(None, Overrides::default(), cx);
    cx.global::<Shared>().0.clone()
}

/// The host settings every window renders through. Requires [`init`] or
/// [`ensure`] to have run.
pub(crate) fn shared(cx: &App) -> Entity<Settings> {
    cx.global::<Shared>().0.clone()
}

/// The in-app navigation bindings in force, for the keymap: the host
/// settings' if they are initialized, this system's provisional defaults
/// otherwise (before [`init`] runs, as in a test that binds keys without
/// a record).
pub(crate) fn keyboard_of(cx: &App) -> Keyboard {
    cx.try_global::<Shared>()
        .map(|shared| shared.0.read(cx).chosen.keyboard.clone())
        .unwrap_or_default()
}

/// The extra selection keys in force, as [`keyboard_of`] reads the
/// bindings.
pub(crate) fn navigation_of(cx: &App) -> pane_core::NavigationBindings {
    cx.try_global::<Shared>()
        .map(|shared| shared.0.read(cx).chosen.navigation)
        .unwrap_or_default()
}

/// How strict root search's matching is in force, as [`keyboard_of`]
/// reads the settings: the launcher applies it on its next keystroke.
pub(crate) fn search_sensitivity_of(cx: &App) -> pane_core::SearchSensitivity {
    cx.try_global::<Shared>()
        .map(|shared| shared.0.read(cx).chosen.search_sensitivity)
        .unwrap_or_default()
}

/// Whether root search learns from what the user chooses in force, as
/// [`keyboard_of`] reads the settings: the launcher applies it as the
/// query changes and as it is made.
pub(crate) fn learning_of(cx: &App) -> bool {
    cx.try_global::<Shared>()
        .map(|shared| shared.0.read(cx).chosen.learning)
        .unwrap_or(true)
}

/// Attaches the launcher that owns the window's global-shortcut
/// registration, and applies the recorded Open Pane hotkey through it:
/// the binding Pane starts with, whatever the record holds. Called by the
/// launcher window's constructor — the first window to exist — and
/// harmlessly again by any later one, over the same launcher whose
/// binding is already applied. A choice that cannot be registered (the
/// system refuses it, or another command's recorded hotkey has it) is
/// kept as the record's choice with the reason as its problem, for the
/// General page to explain.
pub(crate) fn attach_launcher(launcher: &Launcher, cx: &mut App) {
    let settings = ensure(cx);
    let recorded = settings.read(cx).open_pane();
    settings.update(cx, |settings, _| {
        settings.launcher = Some(launcher.clone());
    });
    // The application-owned binding, registered exactly as the command
    // hotkeys are — through the platform adapter the launcher holds, on
    // the window's thread. The outcome is the binding's own state (the
    // problem), not a status the launcher surfaces.
    let _ = launcher.sync_open_pane(recorded);
}

/// Attaches the adapter that shows and hides the native tray or
/// menu-bar entry, and applies the recorded visibility through it: the
/// entry Pane starts with, whatever the record holds — shown, unless the
/// user hid it. Called once, at startup, after [`init`], as the binary
/// does; the tests attach theirs the same way. A choice that cannot be
/// applied — a system where the entry cannot be used at all, or one the
/// system refuses to show — stays the record's choice, with the reason
/// kept as the entry's problem for the General page to explain, as the
/// Open Pane hotkey's refusal is. No entry is made on a platform whose
/// adapter says it has none: the choice stays recorded, and the page
/// explains.
pub fn attach_tray(tray: Arc<dyn pane_core::tray::Tray>, cx: &mut App) {
    let settings = ensure(cx);
    let recorded = settings.read(cx).tray_visible();
    settings.update(cx, |settings, _| {
        settings.tray = Some(tray);
    });
    // The entry the record names is shown now, on the window's thread the
    // adapter is applied from; a refusal keeps the choice, with the reason
    // as the entry's problem for the General page — the outcome is the
    // entry's own state, not a status to surface here, as the Open Pane
    // binding's startup application is not.
    settings.update(cx, |settings, cx| {
        let applied = settings
            .tray
            .as_ref()
            .map(|tray| tray.set_visible(recorded))
            .unwrap_or(Ok(()));
        if let Err(error) = applied {
            settings.tray_problem = Some(error.to_string());
            cx.notify();
        }
    });
}

/// The wiring every Pane window takes around the host settings: its
/// background starts as the material in effect asks, both it and the
/// window's frames follow every change (what the Appearance page chooses
/// repaints the window without a restart), and the platform's appearance
/// notification feeds the system's appearance back into the settings, so
/// a theme that follows the system re-renders when it changes. Call in a
/// window's constructor, over the settings [`ensure`] hands back.
pub(crate) fn bind_window_appearance<T: 'static>(
    settings: &Entity<Settings>,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    window.set_background_appearance(settings.read(cx).material().window_appearance());
    cx.observe_in(settings, window, |_, settings, window, cx| {
        window.set_background_appearance(settings.read(cx).material().window_appearance());
        cx.notify();
    })
    .detach();
    cx.observe_window_appearance(window, {
        let settings = settings.clone();
        move |_, window, cx| {
            let appearance = appearance_of(window.appearance());
            settings.update(cx, |settings, cx| {
                settings.system_appearance(appearance, cx)
            });
        }
    })
    .detach();
}

/// The visuals every window renders with: the theme and material the host
/// settings resolve to right now. A window's render reads this each frame;
/// what keeps a window following changes is the observer its constructor
/// registers (see the module docs), which re-renders it.
pub(crate) fn visuals(cx: &App) -> Visuals {
    shared(cx).read(cx).effective.clone()
}

/// The visuals the launcher window renders with: [`visuals`], over the
/// background image's backdrop when one is chosen and ready (ADR 0028) —
/// the palette then painted over its canvas, with the frosted surfaces
/// (`Theme::over_backdrop`). Everything the launcher draws reads these;
/// the Settings window reads [`visuals`]. The
/// launcher asks for the backdrop as it draws, through
/// [`Settings::request_backdrop`].
pub(crate) fn launcher_visuals(cx: &App) -> Visuals {
    let settings = shared(cx);
    let settings = settings.read(cx);
    let mut visuals = settings.effective.clone();
    if let Some(backdrop) = settings.backdrop() {
        visuals.theme = visuals.theme.over_backdrop(backdrop.canvas);
        visuals.backdrop = Some(backdrop.clone());
    }
    visuals
}

/// The window background appearance the material in effect asks for, for
/// a window about to open: blurred behind a glass panel on the
/// frost-capable platforms, opaque otherwise and for the solid material.
/// Ensures the settings exist first, so a window opened before any
/// initialization still gets a background.
pub(crate) fn window_background(cx: &mut App) -> WindowBackgroundAppearance {
    ensure(cx).read(cx).material().window_appearance()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    /// The solid panel of one of the two palettes, to tell them apart.
    fn panel_of(appearance: Appearance) -> gpui::Hsla {
        Theme::new(appearance).panel_solid
    }

    #[gpui::test]
    fn a_system_appearance_change_re_resolves_a_following_theme(cx: &mut TestAppContext) {
        // A record that follows the system.
        let data = tempfile::tempdir().unwrap();
        std::fs::write(
            data.path().join("settings.json"),
            r#"{ "version": 1, "theme": "system" }"#,
        )
        .unwrap();
        // The environment's overrides are named outright, so nothing a
        // shell carries can change what this test initializes.
        cx.update(|cx| init_with_overrides(Some(data.path().to_owned()), Overrides::default(), cx));
        let settings = cx.update(|cx| shared(cx));

        // The system the test platform reports is light, so that is what
        // a following theme renders.
        assert_eq!(
            cx.update(|cx| visuals(cx).theme.panel_solid),
            panel_of(Appearance::Light)
        );

        // The system switches to dark, as a window's observer reports it
        // (the pane tests cannot drive the platform notification itself;
        // see the appearance page's note in tests/settings.rs) — the
        // entity re-resolves, and the windows observing it repaint.
        settings.update(cx, |settings, cx| {
            settings.system_appearance(Appearance::Dark, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|cx| visuals(cx).theme.panel_solid),
            panel_of(Appearance::Dark)
        );

        // And back.
        settings.update(cx, |settings, cx| {
            settings.system_appearance(Appearance::Light, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|cx| visuals(cx).theme.panel_solid),
            panel_of(Appearance::Light)
        );
    }

    #[gpui::test]
    fn an_unreadable_record_refuses_choices_and_is_not_replaced(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let garbage = "{ not the settings record";
        std::fs::write(data.path().join("settings.json"), garbage).unwrap();
        // The environment's overrides are named outright, so nothing a
        // shell carries can change what this test initializes.
        cx.update(|cx| init_with_overrides(Some(data.path().to_owned()), Overrides::default(), cx));
        let settings = cx.update(|cx| shared(cx));

        // A choice is refused: nothing changes, and the record keeps its
        // source data.
        settings.update(cx, |settings, cx| {
            settings.set_theme(ThemePreference::Light, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| settings.read(cx).theme_preference()),
            ThemePreference::Dark
        );
        assert_eq!(
            std::fs::read_to_string(data.path().join("settings.json")).unwrap(),
            garbage
        );
    }

    #[test]
    fn overrides_parse_the_environment_values() {
        // The theme names its three values; the material its two, with
        // the solid surface written as the old startup variable wrote it.
        assert_eq!(
            Overrides::parse(Some("system"), Some("opaque")),
            Overrides {
                theme: Some(ThemePreference::System),
                material: Some(MaterialPreference::Solid)
            }
        );
        assert_eq!(
            Overrides::parse(Some("light"), Some("glass")),
            Overrides {
                theme: Some(ThemePreference::Light),
                material: Some(MaterialPreference::Glass)
            }
        );
        assert_eq!(
            Overrides::parse(Some("dark"), None),
            Overrides {
                theme: Some(ThemePreference::Dark),
                material: None
            }
        );
        // Unknown values override nothing.
        assert_eq!(
            Overrides::parse(Some("sepia"), Some("frost")),
            Overrides::default()
        );
        assert_eq!(Overrides::parse(None, None), Overrides::default());
    }
}
