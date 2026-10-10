//! The system's session and power commands, as a command reaches them
//! through Pane (`wit/system-commands.wit`, the Windows power features'
//! System Commands, #125, ADR 0040): locking the screen, logging out,
//! restarting, shutting down, sleeping, hibernating, turning the displays
//! off and starting the screen saver, and the audio commands: the volume
//! of the default output device (raising, lowering, setting or muting it)
//! and the microphones' mute, and the bin, appearance and device commands:
//! the Recycle Bin (opening and emptying it), the system's appearance
//! (light and dark), HDR, the desktop, File Explorer's hidden files, the
//! removable drives (ejecting them) and Bluetooth.
//!
//! The decisions are here, as pure functions compiled on every system and
//! tested there: which commands force applications closed (a restart and
//! a shutdown do, a log out does not, the user's choice in ADR 0040),
//! which the extension confirms first, how `sleep` sleeps (a computer
//! that enters Modern Standby when its displays turn off is slept by
//! turning them off, so Windows enters its standby as it does by itself;
//! any other is suspended), whether `hibernate` can happen at all (only
//! with a hibernation file), how far a volume step moves the level,
//! whether the microphone toggle mutes or unmutes (any microphone unmuted
//! means mute), whether the HDR and Bluetooth toggles turn on or off (any
//! capable display or radio off means on), which appearance and
//! hidden-files state the toggles end in, and how the ejection report
//! names the drives that failed. [`run`] turns a decision into calls on
//! the [`SystemCommands`] trait, so an extension cannot get one wrong;
//! each call answers what it ended in ([`Outcome`]) — the state the
//! system is in now, or why nothing changed — never an error, and never a
//! reason to pause the extension.
//!
//! The launcher reaches the system through that one trait, given to it as
//! the system is ([`crate::Launcher::with_system_commands`]): the app
//! passes [`native`], tests a recording fake. A launcher given none
//! answers each command with [`none`]'s explanation. Each adapter is
//! called off the runtime's thread, so it may block for as long as the
//! system takes.
//!
//! - Windows: the documented APIs, in `system_commands/windows.rs`
//!   (`LockWorkStation`; `ExitWindowsEx` with a planned reason and the
//!   shutdown privilege enabled for the call alone; `SetSuspendState`;
//!   `GetPwrCapabilities`; monitor-power and screen-saver messages sent
//!   with a timeout to a window of Pane's own, never a broadcast; Core
//!   Audio's device enumerator and endpoint volume for the volume and
//!   microphone commands; `SHQueryRecycleBinW`, `SHEmptyRecycleBinW` and
//!   the Recycle Bin's known folder for the bin; the personalization
//!   values followed by a setting-change broadcast with a hang timeout
//!   for the appearance; the display configuration's advanced colour
//!   state for HDR; the shell's `ToggleDesktop` for the desktop; File
//!   Explorer's `Hidden` value and the refreshing of its open windows for
//!   the hidden files; the volume's lock and dismount and the setup and
//!   configuration manager's device eject for the drives; and the
//!   Bluetooth radios' service state for Bluetooth).
//! - macOS and Linux: [`native`] answers that the commands are not
//!   available there yet, which the System Commands default extension's
//!   `pane.json` keeps out of those systems' default sets.

use std::sync::Arc;

#[cfg(target_os = "windows")]
mod windows;

/// A session, power or audio command, as the `system-commands` host
/// functions name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Locks the screen; the session stays.
    LockScreen,
    /// Logs the user out, not forcing applications closed.
    LogOut,
    /// Restarts the computer, forcing applications closed.
    Restart,
    /// Powers the computer off, forcing applications closed.
    ShutDown,
    /// Sleeps the computer, as its Modern Standby signal decides.
    Sleep,
    /// Hibernates the computer, which a hibernation file makes possible.
    Hibernate,
    /// Turns the displays off.
    TurnOffDisplays,
    /// Starts the screen saver the user chose.
    StartScreenSaver,
    /// Raises the volume of the default output device.
    VolumeUp,
    /// Lowers the volume of the default output device.
    VolumeDown,
    /// Mutes the default output device when it is not muted, and unmutes
    /// it when it is.
    ToggleMute,
    /// Sets the volume of the default output device to a level, 0 to 100.
    SetVolume(u8),
    /// Mutes every microphone when any is unmuted, unmutes them all
    /// otherwise.
    ToggleMicrophoneMute,
    /// Opens the Recycle Bin.
    OpenRecycleBin,
    /// Empties the Recycle Bin, deleting what it holds for good; an
    /// already empty bin is a success.
    EmptyRecycleBin,
    /// Toggles the system's appearance between light and dark.
    ToggleAppearance,
    /// Toggles HDR over the displays that can show it.
    ToggleHdr,
    /// Shows the desktop, hiding the open windows or bringing them back.
    ShowDesktop,
    /// Toggles whether File Explorer shows hidden files.
    ToggleHiddenFiles,
    /// Ejects every removable drive, reporting each drive that failed.
    EjectRemovableDrives,
    /// Toggles Bluetooth, over the radios there are.
    ToggleBluetooth,
}

impl Command {
    /// Whether it is one of the destructive set, which the System Commands
    /// extension confirms first, destructively, with "Don't ask again"
    /// (ADR 0037's confirm host function, ADR 0040's choice to keep it):
    /// logging out, restarting, shutting down and emptying the Recycle
    /// Bin. A mistyped query never ends a session or empties the bin.
    pub fn destructive(self) -> bool {
        matches!(
            self,
            Command::LogOut | Command::Restart | Command::ShutDown | Command::EmptyRecycleBin
        )
    }

    /// Whether it forces applications closed: a restart and a shutdown do
    /// (the user's choice, ADR 0040, like Raycast), so an application
    /// with unsaved work cannot hold one up; a log out does not, so an
    /// application can still save or veto it.
    pub fn forces_applications_closed(self) -> bool {
        matches!(self, Command::Restart | Command::ShutDown)
    }
}

/// What the computer can do, which decides how some commands act.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Whether the computer enters Modern Standby when its displays turn
    /// off, so `sleep` turns them off rather than suspending.
    pub modern_standby: bool,
    /// Whether the computer has a hibernation file, without which there
    /// is no hibernation.
    pub hibernation_file: bool,
}

/// What `sleep` does on a computer with some [`Capabilities`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepPlan {
    /// Turns the displays off, which puts a Modern Standby computer into
    /// its standby.
    DisplaysOff,
    /// Suspends the computer (S3 sleep).
    Suspend,
}

/// What `hibernate` does on a computer with some [`Capabilities`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HibernatePlan {
    /// Hibernates: the computer has a hibernation file.
    Hibernate,
    /// Nothing changes: there is no hibernation file, without which there
    /// is no hibernation, and the answer says so.
    Explain,
}

/// What `toggle-microphone-mute` does over the microphones there are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicrophonePlan {
    /// Mutes every microphone: at least one is unmuted.
    Mute,
    /// Unmutes every microphone: none is unmuted.
    Unmute,
    /// Nothing changes: there is no microphone, and the answer says so.
    Explain,
}

/// What a toggle turns to over the things it can reach — `toggle-hdr`
/// over the displays that can show it, `toggle-bluetooth` over the
/// radios: on when any of them is off, off when they are all on, and
/// nothing changed when there is none, which the answer explains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TogglePlan {
    /// Turns them all on: at least one is off.
    On,
    /// Turns them all off: none is on.
    Off,
    /// Nothing changes: there is none, and the answer says so.
    Explain,
}

/// The volume of the default output device: its level, 0 to 100, and
/// whether it is muted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Volume {
    /// The level, from silent (0) to full (100).
    pub level: u8,
    /// Whether the output is muted.
    pub muted: bool,
}

/// A microphone (a capture device), as the microphone toggle sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Microphone {
    /// Its id, as the adapter names it: the handle the toggle mutes or
    /// unmutes it by, valid for the session.
    pub id: String,
    /// Whether it is muted.
    pub muted: bool,
}

/// The state of the Recycle Bin: how much it holds. An empty one is a
/// bin `empty-recycle-bin` answers as a success.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecycleBin {
    /// How many items it holds, over every drive.
    pub items: u64,
    /// How much they take up, in bytes.
    pub size: u64,
}

/// The system's appearance, as the personalization values say it: the
/// light mode or the dark one, for applications and the system together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    /// The light mode: applications and the system use light colors.
    #[default]
    Light,
    /// The dark mode: applications and the system use dark colors.
    Dark,
}

/// A display that can show HDR, as the HDR toggle sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HdrDisplay {
    /// Its id, as the adapter names it: the handle the toggle turns its
    /// advanced colour on or off by, valid for the session.
    pub id: String,
    /// Whether its advanced colour is on (HDR is showing).
    pub hdr: bool,
}

/// A Bluetooth radio, as the Bluetooth toggle sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothRadio {
    /// Its id, as the adapter names it: the handle the toggle turns it
    /// on or off by, valid for the session.
    pub id: String,
    /// Whether the radio is on.
    pub on: bool,
}

/// A removable drive, as the ejection sees it: the drive the user sees,
/// such as a USB stick's `E:`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    /// Its id, as the adapter names it: the drive with its colon, such
    /// as "E:", the handle the ejection ejects it by, valid for the
    /// session.
    pub id: String,
}

/// What a command ended in, as the command answers it: the state the
/// system is in now, or why nothing changed (`outcome` in the WIT). The
/// text is what the command shows the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It happened: what the system is doing now, for the HUD.
    Done(String),
    /// Nothing changed: why, for the HUD.
    Explained(String),
}

/// How a session ends, as [`SystemCommands::power_off`] carries it out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerRequest {
    /// Logs the user out; applications are asked to close, not forced.
    LogOut,
    /// Restarts the computer; applications are forced closed.
    Restart,
    /// Powers the computer off; applications are forced closed.
    ShutDown,
}

/// The session, power and audio commands of the system Pane runs on, as
/// the `system-commands` host functions act on it. Each is called off the
/// runtime's thread and may block; an error explains why nothing changed.
/// The decisions are Pane's ([`run`]); the adapter only carries them out.
pub trait SystemCommands: Send + Sync + 'static {
    /// What the computer can do, which decides how some commands act.
    fn capabilities(&self) -> Result<Capabilities, String>;

    /// Locks the screen, keeping the user's session.
    fn lock_screen(&self) -> Result<(), String>;

    /// Ends the user's session: logs the user out, restarts the computer
    /// or powers it off, as `request` says. `force` closes applications
    /// without giving them the chance to save, which the caller decides
    /// per command ([`Command::forces_applications_closed`]); a session
    /// that ends the system needs the shutdown privilege, which the
    /// adapter enables for the call alone.
    fn power_off(&self, request: PowerRequest, force: bool) -> Result<(), String>;

    /// Suspends the computer, or hibernates it when `hibernate` is set
    /// (which only a computer with a hibernation file is asked for).
    fn suspend(&self, hibernate: bool) -> Result<(), String>;

    /// Turns the displays off, which also puts a Modern Standby computer
    /// into its standby.
    fn displays_off(&self) -> Result<(), String>;

    /// Starts the screen saver the user chose, or answers that none is
    /// set.
    fn screen_saver(&self) -> Result<(), String>;

    /// The volume of the default output device: its level, 0 to 100, and
    /// whether it is muted.
    fn volume(&self) -> Result<Volume, String>;

    /// Sets the default output device's volume to `volume`, its level and
    /// its mute together, answering the volume it ended at.
    fn set_volume(&self, volume: Volume) -> Result<Volume, String>;

    /// The capture devices (microphones) there are, each with whether it
    /// is muted.
    fn microphones(&self) -> Result<Vec<Microphone>, String>;

    /// Mutes or unmutes the microphone `id`, as the toggle decided over
    /// the list it was given; a device that has vanished since it was
    /// listed answers why, and the caller skips it.
    fn set_microphone_mute(&self, id: &str, muted: bool) -> Result<(), String>;

    /// The state of the Recycle Bin: how much it holds. An empty one is
    /// one `empty-recycle-bin` answers as a success.
    fn recycle_bin(&self) -> Result<RecycleBin, String>;

    /// Opens the Recycle Bin in the file manager.
    fn open_recycle_bin(&self) -> Result<(), String>;

    /// Empties the Recycle Bin, what it holds deleted for good. The
    /// caller decides what an already empty bin answers.
    fn empty_recycle_bin(&self) -> Result<(), String>;

    /// The system's appearance: the light or the dark mode, as the
    /// personalization values say it.
    fn appearance(&self) -> Result<Appearance, String>;

    /// Sets the system's appearance to `appearance`, the personalization
    /// values for applications and the system together, followed by the
    /// setting-change broadcast with a hang timeout, answering the
    /// appearance it ended in.
    fn set_appearance(&self, appearance: Appearance) -> Result<Appearance, String>;

    /// The displays that can show HDR, each with whether its advanced
    /// colour is on; a display that cannot is not listed.
    fn hdr_displays(&self) -> Result<Vec<HdrDisplay>, String>;

    /// Turns the display `id`'s advanced colour on or off, as the toggle
    /// decided over the list it was given; a display that has vanished
    /// since it was listed answers why.
    fn set_hdr(&self, id: &str, hdr: bool) -> Result<(), String>;

    /// Shows the desktop, as the shell's own toggle-desktop does.
    fn show_desktop(&self) -> Result<(), String>;

    /// Whether the file manager shows hidden files.
    fn hidden_files(&self) -> Result<bool, String>;

    /// Sets whether the file manager shows hidden files, refreshing its
    /// open windows.
    fn set_hidden_files(&self, shown: bool) -> Result<(), String>;

    /// The removable drives there are, such as a USB stick's `E:`.
    fn removable_drives(&self) -> Result<Vec<Drive>, String>;

    /// Ejects the drive `id`, locking it, dismounting it and ejecting
    /// the device behind it; a drive that is in use answers why.
    fn eject_drive(&self, id: &str) -> Result<(), String>;

    /// The Bluetooth radios there are, each with whether it is on.
    fn bluetooth_radios(&self) -> Result<Vec<BluetoothRadio>, String>;

    /// Turns the radio `id` on or off, as the toggle decided over the
    /// list it was given; a radio that has vanished since it was listed
    /// answers why.
    fn set_bluetooth(&self, id: &str, on: bool) -> Result<(), String>;
}

/// Why `hibernate` answers when the computer has no hibernation file.
pub const NO_HIBERNATION_FILE: &str =
    "Hibernation is not available on this computer: there is no hibernation file";

/// Why `set-volume` answers when its level is not 0 to 100.
pub const SET_VOLUME_RANGE: &str = "The volume can be set only from 0 to 100";

/// Why `toggle-microphone-mute` answers when there is no microphone.
pub const NO_MICROPHONE: &str = "No microphone is connected";

/// Why `toggle-hdr` answers when no display can show HDR.
pub const NO_HDR: &str = "No HDR-capable display is connected";

/// Why `eject-removable-drives` answers when there is no removable
/// drive.
pub const NO_REMOVABLE_DRIVE: &str = "No removable drive is connected";

/// Why `toggle-bluetooth` answers when there is no Bluetooth radio.
pub const NO_BLUETOOTH: &str = "No Bluetooth radio is connected";

/// What `sleep` says when it happened.
const SLEEPING: &str = "Sleeping";
/// What `hibernate` says when it happened.
const HIBERNATING: &str = "Hibernating";
/// What `toggle-mute` says when it muted the output.
const MUTED: &str = "Muted";
/// What the microphone toggle says when it muted every microphone.
const MICROPHONES_MUTED: &str = "Microphones muted";
/// What the microphone toggle says when it unmuted every microphone.
const MICROPHONES_UNMUTED: &str = "Microphones unmuted";
/// What `empty-recycle-bin` says when the bin was already empty, which
/// is a success.
const BIN_ALREADY_EMPTY: &str = "The Recycle Bin is already empty";
/// What the appearance toggle says when it ended in the light mode.
const LIGHT_MODE: &str = "Light mode";
/// What the appearance toggle says when it ended in the dark mode.
const DARK_MODE: &str = "Dark mode";
/// What the HDR toggle says when it turned HDR on.
const HDR_ON: &str = "HDR on";
/// What the HDR toggle says when it turned HDR off.
const HDR_OFF: &str = "HDR off";
/// What the hidden-files toggle says when files are shown.
const HIDDEN_FILES_SHOWN: &str = "Hidden files shown";
/// What the hidden-files toggle says when files are hidden.
const HIDDEN_FILES_HIDDEN: &str = "Hidden files hidden";
/// What the Bluetooth toggle says when it turned the radios on.
const BLUETOOTH_ON: &str = "Bluetooth on";
/// What the Bluetooth toggle says when it turned the radios off.
const BLUETOOTH_OFF: &str = "Bluetooth off";
/// The level a volume can be set to at most.
const FULL_VOLUME: u8 = 100;
/// How far a volume step moves the level, of 100: the step Windows' own
/// volume keys take.
const VOLUME_STEP: u8 = 2;

/// What `sleep` does on a computer with `capabilities`: one that enters
/// Modern Standby when its displays turn off is slept by turning them off,
/// so Windows enters its standby as it does by itself; any other is
/// suspended.
pub fn sleep_plan(capabilities: &Capabilities) -> SleepPlan {
    if capabilities.modern_standby {
        SleepPlan::DisplaysOff
    } else {
        SleepPlan::Suspend
    }
}

/// What `hibernate` does on a computer with `capabilities`: only one with
/// a hibernation file can hibernate; without one, nothing changes and the
/// answer says so.
pub fn hibernate_plan(capabilities: &Capabilities) -> HibernatePlan {
    if capabilities.hibernation_file {
        HibernatePlan::Hibernate
    } else {
        HibernatePlan::Explain
    }
}

/// The volume `volume-up` ends at from `now`: one step up
/// ([`VOLUME_STEP`], as Windows' own volume keys step), never past full,
/// with the mute as it was.
pub fn volume_up(now: Volume) -> Volume {
    Volume {
        level: now.level.saturating_add(VOLUME_STEP).min(FULL_VOLUME),
        ..now
    }
}

/// The volume `volume-down` ends at from `now`: one step down, never
/// below silent, with the mute as it was.
pub fn volume_down(now: Volume) -> Volume {
    Volume {
        level: now.level.saturating_sub(VOLUME_STEP),
        ..now
    }
}

/// The volume `toggle-mute` ends at from `now`: the device's mute,
/// flipped, with the level as it was.
pub fn toggled_mute(now: Volume) -> Volume {
    Volume {
        muted: !now.muted,
        ..now
    }
}

/// What `toggle-microphone-mute` does over `microphones`: mutes them all
/// when any is unmuted, unmutes them all otherwise; none at all is
/// explained.
pub fn microphone_plan(microphones: &[Microphone]) -> MicrophonePlan {
    if microphones.is_empty() {
        MicrophonePlan::Explain
    } else if microphones.iter().any(|device| !device.muted) {
        MicrophonePlan::Mute
    } else {
        MicrophonePlan::Unmute
    }
}

/// The appearance `toggle-appearance` ends at from `now`: the other
/// mode, for applications and the system together.
pub fn toggled_appearance(now: Appearance) -> Appearance {
    match now {
        Appearance::Light => Appearance::Dark,
        Appearance::Dark => Appearance::Light,
    }
}

/// What `toggle-hdr` does over `displays`, the ones that can show it:
/// turns them all on when any is off, all off when none is; none at all
/// is explained.
pub fn hdr_plan(displays: &[HdrDisplay]) -> TogglePlan {
    if displays.is_empty() {
        TogglePlan::Explain
    } else if displays.iter().any(|display| !display.hdr) {
        TogglePlan::On
    } else {
        TogglePlan::Off
    }
}

/// What `toggle-bluetooth` does over `radios`: turns them all on when
/// any is off, all off when none is; none at all is explained.
pub fn bluetooth_plan(radios: &[BluetoothRadio]) -> TogglePlan {
    if radios.is_empty() {
        TogglePlan::Explain
    } else if radios.iter().any(|radio| !radio.on) {
        TogglePlan::On
    } else {
        TogglePlan::Off
    }
}

/// What `command` does through `commands`: the state it ended in, or why
/// nothing changed. The decisions are Pane's — which commands force
/// applications closed, how `sleep` sleeps, whether `hibernate` can
/// happen — so an extension cannot get one wrong, whatever system it runs
/// on.
pub fn run(command: Command, commands: &dyn SystemCommands) -> Outcome {
    let answer = match command {
        Command::LockScreen => commands.lock_screen(),
        Command::LogOut | Command::Restart | Command::ShutDown => {
            commands.power_off(power_request(command), command.forces_applications_closed())
        }
        Command::Sleep => return sleep(commands),
        Command::Hibernate => return hibernate(commands),
        Command::VolumeUp => return changed(commands, volume_up, volume_text),
        Command::VolumeDown => return changed(commands, volume_down, volume_text),
        Command::ToggleMute => return changed(commands, toggled_mute, mute_text),
        Command::SetVolume(level) => return set_level(level, commands),
        Command::ToggleMicrophoneMute => return toggle_microphones(commands),
        Command::OpenRecycleBin => commands.open_recycle_bin(),
        Command::EmptyRecycleBin => return empty_bin(commands),
        Command::ToggleAppearance => return toggle_appearance(commands),
        Command::ToggleHdr => return toggle_hdr(commands),
        Command::ShowDesktop => commands.show_desktop(),
        Command::ToggleHiddenFiles => return toggle_hidden_files(commands),
        Command::EjectRemovableDrives => return eject_drives(commands),
        Command::ToggleBluetooth => return toggle_bluetooth(commands),
        Command::TurnOffDisplays => commands.displays_off(),
        Command::StartScreenSaver => commands.screen_saver(),
    };
    ended(answer, done_text(command))
}

/// How the session-ending `command` ends it.
fn power_request(command: Command) -> PowerRequest {
    match command {
        Command::LogOut => PowerRequest::LogOut,
        Command::Restart => PowerRequest::Restart,
        _ => PowerRequest::ShutDown,
    }
}

/// What `command` says when it happened, for the HUD.
fn done_text(command: Command) -> &'static str {
    match command {
        Command::LockScreen => "Locking the screen",
        Command::LogOut => "Logging out",
        Command::Restart => "Restarting",
        Command::ShutDown => "Shutting down",
        Command::Sleep => SLEEPING,
        Command::Hibernate => HIBERNATING,
        Command::TurnOffDisplays => "Turning off the displays",
        Command::StartScreenSaver => "Starting the screen saver",
        Command::OpenRecycleBin => "Opening the Recycle Bin",
        Command::EmptyRecycleBin => "Emptied the Recycle Bin",
        Command::ShowDesktop => "Showing the desktop",
        // The volume and microphone commands answer with the state they
        // ended in, which the adapter says ([`volume_text`],
        // [`mute_text`]); they never reach a fixed text. So do the
        // appearance, HDR, hidden-files, drive and Bluetooth ones, and
        // an already empty Recycle Bin, which the decisions say.
        Command::VolumeUp
        | Command::VolumeDown
        | Command::ToggleMute
        | Command::SetVolume(_)
        | Command::ToggleMicrophoneMute
        | Command::ToggleAppearance
        | Command::ToggleHdr
        | Command::ToggleHiddenFiles
        | Command::EjectRemovableDrives
        | Command::ToggleBluetooth => "",
    }
}

/// `answer` as the outcome of a command that says `text` when it happens.
fn ended(answer: Result<(), String>, text: &str) -> Outcome {
    match answer {
        Ok(()) => Outcome::Done(text.into()),
        Err(why) => Outcome::Explained(why),
    }
}

/// What `sleep` does through `commands`: as its Modern Standby signal
/// decides ([`sleep_plan`]), turning the displays off or suspending.
fn sleep(commands: &dyn SystemCommands) -> Outcome {
    let capabilities = match commands.capabilities() {
        Ok(capabilities) => capabilities,
        Err(why) => return Outcome::Explained(why),
    };
    let answer = match sleep_plan(&capabilities) {
        SleepPlan::DisplaysOff => commands.displays_off(),
        SleepPlan::Suspend => commands.suspend(false),
    };
    ended(answer, SLEEPING)
}

/// What `hibernate` does through `commands`: only a computer with a
/// hibernation file hibernates ([`hibernate_plan`]); without one nothing
/// changes and the answer says why.
fn hibernate(commands: &dyn SystemCommands) -> Outcome {
    let capabilities = match commands.capabilities() {
        Ok(capabilities) => capabilities,
        Err(why) => return Outcome::Explained(why),
    };
    match hibernate_plan(&capabilities) {
        HibernatePlan::Hibernate => ended(commands.suspend(true), HIBERNATING),
        HibernatePlan::Explain => Outcome::Explained(NO_HIBERNATION_FILE.into()),
    }
}

/// What a volume command says when it happened: the volume it ended at.
fn volume_text(volume: Volume) -> String {
    format!("Volume {}%", volume.level)
}

/// What `toggle-mute` says when it happened: which it did, with the
/// volume it ended at when it unmuted.
fn mute_text(volume: Volume) -> String {
    if volume.muted {
        MUTED.into()
    } else {
        format!("Unmuted, {}", volume_text(volume))
    }
}

/// What a volume command does through `commands`: reads the volume,
/// changes it to what `change` says, and answers the volume it ended at
/// as `text` says.
fn changed(
    commands: &dyn SystemCommands,
    change: impl FnOnce(Volume) -> Volume,
    text: fn(Volume) -> String,
) -> Outcome {
    let now = match commands.volume() {
        Ok(now) => now,
        Err(why) => return Outcome::Explained(why),
    };
    match commands.set_volume(change(now)) {
        Ok(ended) => Outcome::Done(text(ended)),
        Err(why) => Outcome::Explained(why),
    }
}

/// What `set-volume` does through `commands`: sets the level it is given,
/// keeping the mute as it is ([`at_level`]), and answers the volume it
/// ended at. A level that is not 0 to 100 changes nothing and the answer
/// says why.
fn set_level(level: u8, commands: &dyn SystemCommands) -> Outcome {
    if level > FULL_VOLUME {
        return Outcome::Explained(SET_VOLUME_RANGE.into());
    }
    changed(commands, at_level(level), volume_text)
}

/// The change `set-volume` makes to the volume it finds: the level given,
/// with the mute as it was.
fn at_level(level: u8) -> impl Fn(Volume) -> Volume {
    move |now| Volume { level, ..now }
}

/// What `toggle-microphone-mute` does through `commands`: mutes every
/// microphone when any is unmuted and unmutes them all otherwise
/// ([`microphone_plan`]), skipping devices that vanish meanwhile; no
/// microphone at all is explained.
fn toggle_microphones(commands: &dyn SystemCommands) -> Outcome {
    let microphones = match commands.microphones() {
        Ok(microphones) => microphones,
        Err(why) => return Outcome::Explained(why),
    };
    match microphone_plan(&microphones) {
        MicrophonePlan::Mute => set_each(&microphones, true, MICROPHONES_MUTED, commands),
        MicrophonePlan::Unmute => set_each(&microphones, false, MICROPHONES_UNMUTED, commands),
        MicrophonePlan::Explain => Outcome::Explained(NO_MICROPHONE.into()),
    }
}

/// Sets every microphone in `devices` to `muted`, skipping one that has
/// vanished meanwhile (its answer says why), and answers `text` when any
/// was set; when none was, the last vanishing says why nothing changed.
fn set_each(
    devices: &[Microphone],
    muted: bool,
    text: &str,
    commands: &dyn SystemCommands,
) -> Outcome {
    set_all(
        devices,
        NO_MICROPHONE,
        |device| commands.set_microphone_mute(&device.id, muted),
        text,
    )
}

/// Sets every item of `devices` as `set` says, skipping one that fails
/// (its answer says why), and answers `text` when any was set; when none
/// was, the last failure says why nothing changed, or `none` when there
/// was no failure at all (there was no device).
fn set_all<T>(
    devices: &[T],
    none: &str,
    mut set: impl FnMut(&T) -> Result<(), String>,
    text: &str,
) -> Outcome {
    let mut changed = 0;
    let mut failed: Option<String> = None;
    for device in devices {
        match set(device) {
            Ok(()) => changed += 1,
            Err(why) => failed = Some(why),
        }
    }
    if changed > 0 {
        Outcome::Done(text.into())
    } else {
        Outcome::Explained(failed.unwrap_or_else(|| none.into()))
    }
}

/// What `empty-recycle-bin` does through `commands`: asks what the bin
/// holds; an already empty one is a success the answer says, and one
/// that holds something is emptied. The confirmation is the extension's
/// ([`Command::destructive`]).
fn empty_bin(commands: &dyn SystemCommands) -> Outcome {
    let bin = match commands.recycle_bin() {
        Ok(bin) => bin,
        Err(why) => return Outcome::Explained(why),
    };
    if bin.items == 0 {
        return Outcome::Done(BIN_ALREADY_EMPTY.into());
    }
    ended(
        commands.empty_recycle_bin(),
        done_text(Command::EmptyRecycleBin),
    )
}

/// What `toggle-appearance` does through `commands`: reads the
/// appearance, sets the other one ([`toggled_appearance`]), and answers
/// the mode it ended in.
fn toggle_appearance(commands: &dyn SystemCommands) -> Outcome {
    let now = match commands.appearance() {
        Ok(now) => now,
        Err(why) => return Outcome::Explained(why),
    };
    match commands.set_appearance(toggled_appearance(now)) {
        Ok(appearance) => Outcome::Done(appearance_text(appearance)),
        Err(why) => Outcome::Explained(why),
    }
}

/// What the appearance toggle says when it happened: the mode it ended
/// in.
fn appearance_text(appearance: Appearance) -> String {
    match appearance {
        Appearance::Light => LIGHT_MODE.into(),
        Appearance::Dark => DARK_MODE.into(),
    }
}

/// What `toggle-hdr` does through `commands`: turns every capable
/// display on when any is off and all off when none is ([`hdr_plan`]),
/// skipping a display that vanishes meanwhile; no display at all is
/// explained.
fn toggle_hdr(commands: &dyn SystemCommands) -> Outcome {
    let displays = match commands.hdr_displays() {
        Ok(displays) => displays,
        Err(why) => return Outcome::Explained(why),
    };
    match hdr_plan(&displays) {
        TogglePlan::On => set_all(
            &displays,
            NO_HDR,
            |display| commands.set_hdr(&display.id, true),
            HDR_ON,
        ),
        TogglePlan::Off => set_all(
            &displays,
            NO_HDR,
            |display| commands.set_hdr(&display.id, false),
            HDR_OFF,
        ),
        TogglePlan::Explain => Outcome::Explained(NO_HDR.into()),
    }
}

/// What `toggle-hidden-files` does through `commands`: reads whether the
/// file manager shows hidden files, sets the other one, and answers
/// which it ended in.
fn toggle_hidden_files(commands: &dyn SystemCommands) -> Outcome {
    let shown = match commands.hidden_files() {
        Ok(shown) => shown,
        Err(why) => return Outcome::Explained(why),
    };
    match commands.set_hidden_files(!shown) {
        Ok(()) => Outcome::Done(hidden_files_text(!shown)),
        Err(why) => Outcome::Explained(why),
    }
}

/// What the hidden-files toggle says when it happened: which it chose.
fn hidden_files_text(shown: bool) -> String {
    if shown {
        HIDDEN_FILES_SHOWN.into()
    } else {
        HIDDEN_FILES_HIDDEN.into()
    }
}

/// What `eject-removable-drives` does through `commands`: ejects each
/// removable drive, and the report names the ones that refused with why;
/// no removable drive at all is explained.
fn eject_drives(commands: &dyn SystemCommands) -> Outcome {
    let drives = match commands.removable_drives() {
        Ok(drives) => drives,
        Err(why) => return Outcome::Explained(why),
    };
    if drives.is_empty() {
        return Outcome::Explained(NO_REMOVABLE_DRIVE.into());
    }
    let mut ejected: Vec<&str> = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    for drive in &drives {
        match commands.eject_drive(&drive.id) {
            Ok(()) => ejected.push(&drive.id),
            Err(why) => refused.push(format!("{} was not ejected: {}", drive.id, why)),
        }
    }
    if ejected.is_empty() {
        return Outcome::Explained(refused.join("; "));
    }
    let mut report = format!("Ejected {}", ejected.join(", "));
    if !refused.is_empty() {
        report.push_str("; ");
        report.push_str(&refused.join("; "));
    }
    Outcome::Done(report)
}

/// What `toggle-bluetooth` does through `commands`: turns every radio on
/// when any is off and all off when none is ([`bluetooth_plan`]),
/// skipping a radio that vanishes meanwhile; no radio at all is
/// explained.
fn toggle_bluetooth(commands: &dyn SystemCommands) -> Outcome {
    let radios = match commands.bluetooth_radios() {
        Ok(radios) => radios,
        Err(why) => return Outcome::Explained(why),
    };
    match bluetooth_plan(&radios) {
        TogglePlan::On => set_all(
            &radios,
            NO_BLUETOOTH,
            |radio| commands.set_bluetooth(&radio.id, true),
            BLUETOOTH_ON,
        ),
        TogglePlan::Off => set_all(
            &radios,
            NO_BLUETOOTH,
            |radio| commands.set_bluetooth(&radio.id, false),
            BLUETOOTH_OFF,
        ),
        TogglePlan::Explain => Outcome::Explained(NO_BLUETOOTH.into()),
    }
}

/// This system's adapter: Windows', or one that explains that the
/// session and power commands are Windows-only for now.
pub fn native() -> Arc<dyn SystemCommands> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsSystemCommands)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(Unavailable(format!(
            "Not available on {}: the session and power commands are Windows-only for now",
            system_name()
        )))
    }
}

/// The commands of a launcher given none: every command answers that this
/// Pane reaches no session and power commands.
pub fn none() -> Arc<dyn SystemCommands> {
    Arc::new(Unavailable(
        "Not available: this Pane reaches no session and power commands".into(),
    ))
}

/// This system's name as a sentence says it.
#[cfg(not(target_os = "windows"))]
fn system_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        std::env::consts::OS
    }
}

/// A system Pane reaches no session and power commands of, saying why.
struct Unavailable(String);

impl SystemCommands for Unavailable {
    fn capabilities(&self) -> Result<Capabilities, String> {
        Err(self.0.clone())
    }

    fn lock_screen(&self) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn power_off(&self, _request: PowerRequest, _force: bool) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn suspend(&self, _hibernate: bool) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn displays_off(&self) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn screen_saver(&self) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn volume(&self) -> Result<Volume, String> {
        Err(self.0.clone())
    }

    fn set_volume(&self, _volume: Volume) -> Result<Volume, String> {
        Err(self.0.clone())
    }

    fn microphones(&self) -> Result<Vec<Microphone>, String> {
        Err(self.0.clone())
    }

    fn set_microphone_mute(&self, _id: &str, _muted: bool) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn recycle_bin(&self) -> Result<RecycleBin, String> {
        Err(self.0.clone())
    }

    fn open_recycle_bin(&self) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn empty_recycle_bin(&self) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn appearance(&self) -> Result<Appearance, String> {
        Err(self.0.clone())
    }

    fn set_appearance(&self, _appearance: Appearance) -> Result<Appearance, String> {
        Err(self.0.clone())
    }

    fn hdr_displays(&self) -> Result<Vec<HdrDisplay>, String> {
        Err(self.0.clone())
    }

    fn set_hdr(&self, _id: &str, _hdr: bool) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn show_desktop(&self) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn hidden_files(&self) -> Result<bool, String> {
        Err(self.0.clone())
    }

    fn set_hidden_files(&self, _shown: bool) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn removable_drives(&self) -> Result<Vec<Drive>, String> {
        Err(self.0.clone())
    }

    fn eject_drive(&self, _id: &str) -> Result<(), String> {
        Err(self.0.clone())
    }

    fn bluetooth_radios(&self) -> Result<Vec<BluetoothRadio>, String> {
        Err(self.0.clone())
    }

    fn set_bluetooth(&self, _id: &str, _on: bool) -> Result<(), String> {
        Err(self.0.clone())
    }
}
