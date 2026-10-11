//! A fake of the system's session, power, audio, bin, appearance and
//! device commands for the tests (#255, #265, #266): it answers what the
//! test sets and records what it was asked to do, so a test never locks,
//! logs out, restarts, shuts down, sleeps, hibernates, turns off the
//! displays of a real session, changes the volume or mutes a microphone,
//! empties the Recycle Bin, changes the appearance or the hidden files,
//! ejects a drive or toggles Bluetooth. The default is a computer that
//! suspends (not Modern Standby) and has a hibernation file, with the
//! volume at 50 and unmuted and one unmuted microphone, a Recycle Bin
//! holding three items (1024 bytes), the light mode, one HDR-capable
//! display with HDR off, hidden files hidden, one removable drive and
//! one Bluetooth radio on, and every call succeeds;
//! `set_capabilities`, `set_volume_state`, `set_microphones`, `vanish`,
//! `set_bin`, `set_hdr_displays`, `set_drives`, `busy`,
//! `set_bluetooth_radios` and `fail` change that.

#![allow(dead_code)]

use std::sync::Mutex;

use pane_core::system_commands::{
    Appearance, BluetoothRadio, Capabilities, Drive, HdrDisplay, Microphone, PowerRequest,
    RecycleBin, SystemCommands, Volume,
};

/// What the fake was asked to do, in order: each adapter call, with how it
/// was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Done {
    /// Locked the screen.
    Locked,
    /// Ended the session, with how it ends it and whether applications
    /// are forced closed.
    Power(PowerRequest, bool),
    /// Suspended the computer, or hibernated it.
    Suspended(bool),
    /// Turned the displays off.
    DisplaysOff,
    /// Started the screen saver.
    ScreenSaver,
    /// Set the default output device's volume, to the volume given.
    SetVolume(Volume),
    /// Set the microphone's mute, by its id, to the mute given.
    SetMicrophoneMute(String, bool),
    /// Opened the Recycle Bin.
    OpenedBin,
    /// Emptied the Recycle Bin.
    EmptiedBin,
    /// Set the system's appearance, to the mode given.
    SetAppearance(Appearance),
    /// Set the display's advanced colour, by its id, to the state given.
    SetHdr(String, bool),
    /// Showed the desktop.
    ShowedDesktop,
    /// Set whether the file manager shows hidden files, to the state
    /// given.
    SetHiddenFiles(bool),
    /// Ejected the drive, by its id.
    Ejected(String),
    /// Set the Bluetooth radio's state, by its id, to the state given.
    SetBluetooth(String, bool),
}

/// What a vanished microphone answers when it is set, as one that went
/// since it was listed does.
const GONE: &str = "The microphone is gone";

/// What a removable drive that is in use answers when it is ejected.
const BUSY: &str = "The drive is in use";

/// What a vanished Bluetooth radio answers when it is set, as one that
/// went since it was listed does.
const RADIO_GONE: &str = "The Bluetooth radio is gone";

/// The session, power and audio commands, recorded: what `capabilities`,
/// `volume` and `microphones` say is set by the test, and so is any
/// failure. It starts as a computer that suspends (not Modern Standby)
/// and has a hibernation file, with the volume at 50 and unmuted and one
/// unmuted microphone, with every call succeeding.
pub struct RecordingSystemCommands {
    done: Mutex<Vec<Done>>,
    capabilities: Mutex<Capabilities>,
    failure: Mutex<Option<String>>,
    volume: Mutex<Volume>,
    microphones: Mutex<Vec<Microphone>>,
    vanished: Mutex<Vec<String>>,
    bin: Mutex<RecycleBin>,
    appearance: Mutex<Appearance>,
    hdr: Mutex<Vec<HdrDisplay>>,
    hidden_files: Mutex<bool>,
    drives: Mutex<Vec<Drive>>,
    busy: Mutex<Vec<String>>,
    bluetooth: Mutex<Vec<BluetoothRadio>>,
    gone_radios: Mutex<Vec<String>>,
}

impl Default for RecordingSystemCommands {
    fn default() -> RecordingSystemCommands {
        RecordingSystemCommands {
            done: Mutex::new(Vec::new()),
            capabilities: Mutex::new(Capabilities {
                modern_standby: false,
                hibernation_file: true,
            }),
            failure: Mutex::new(None),
            volume: Mutex::new(Volume {
                level: 50,
                muted: false,
            }),
            microphones: Mutex::new(vec![Microphone {
                id: "microphone".into(),
                muted: false,
            }]),
            vanished: Mutex::new(Vec::new()),
            bin: Mutex::new(RecycleBin {
                items: 3,
                size: 1024,
            }),
            appearance: Mutex::new(Appearance::Light),
            hdr: Mutex::new(vec![HdrDisplay {
                id: "display".into(),
                hdr: false,
            }]),
            hidden_files: Mutex::new(false),
            drives: Mutex::new(vec![Drive { id: "E:".into() }]),
            busy: Mutex::new(Vec::new()),
            bluetooth: Mutex::new(vec![BluetoothRadio {
                id: "radio".into(),
                on: true,
            }]),
            gone_radios: Mutex::new(Vec::new()),
        }
    }
}

impl RecordingSystemCommands {
    /// What the fake was asked to do so far, in order, forgotten once read.
    pub fn take(&self) -> Vec<Done> {
        std::mem::take(&mut *self.done.lock().unwrap())
    }

    /// What `capabilities` answers from now on.
    pub fn set_capabilities(&self, capabilities: Capabilities) {
        *self.capabilities.lock().unwrap() = capabilities;
    }

    /// What `volume` answers from now on.
    pub fn set_volume_state(&self, volume: Volume) {
        *self.volume.lock().unwrap() = volume;
    }

    /// The microphones there are from now on, each with whether it is
    /// muted; none at all answers that no microphone is connected.
    pub fn set_microphones(&self, microphones: Vec<Microphone>) {
        *self.microphones.lock().unwrap() = microphones;
    }

    /// The microphone `id` answers that it is gone from now on, as one
    /// that vanished since it was listed does.
    pub fn vanish(&self, id: &str) {
        self.vanished.lock().unwrap().push(id.into());
    }

    /// What the Recycle Bin holds from now on: an empty one (0 items) is
    /// a bin emptying answers as a success.
    pub fn set_bin(&self, bin: RecycleBin) {
        *self.bin.lock().unwrap() = bin;
    }

    /// The displays that can show HDR from now on, each with whether its
    /// advanced colour is on; none at all answers that no display can.
    pub fn set_hdr_displays(&self, displays: Vec<HdrDisplay>) {
        *self.hdr.lock().unwrap() = displays;
    }

    /// The removable drives there are from now on; none at all answers
    /// that no removable drive is connected.
    pub fn set_drives(&self, drives: Vec<Drive>) {
        *self.drives.lock().unwrap() = drives;
    }

    /// The drive `id` answers that it is in use from now on, when it is
    /// ejected.
    pub fn busy(&self, id: &str) {
        self.busy.lock().unwrap().push(id.into());
    }

    /// The Bluetooth radios there are from now on, each with whether it
    /// is on; none at all answers that no radio is connected.
    pub fn set_bluetooth_radios(&self, radios: Vec<BluetoothRadio>) {
        *self.bluetooth.lock().unwrap() = radios;
    }

    /// The Bluetooth radio `id` answers that it is gone from now on, as
    /// one that vanished since it was listed does.
    pub fn vanish_radio(&self, id: &str) {
        self.gone_radios.lock().unwrap().push(id.into());
    }

    /// Every call fails with `why` from now on.
    pub fn fail(&self, why: &str) {
        *self.failure.lock().unwrap() = Some(why.into());
    }

    /// `answer` as the fake answers it, recording `done` when it
    /// succeeds.
    fn answer(&self, done: Done, result: Result<(), String>) -> Result<(), String> {
        match (&*self.failure.lock().unwrap(), result) {
            (Some(why), _) => Err(why.clone()),
            (None, Ok(())) => {
                self.done.lock().unwrap().push(done);
                Ok(())
            }
            (None, Err(why)) => Err(why),
        }
    }
}

impl SystemCommands for RecordingSystemCommands {
    fn capabilities(&self) -> Result<Capabilities, String> {
        match self.failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(*self.capabilities.lock().unwrap()),
        }
    }

    fn lock_screen(&self) -> Result<(), String> {
        self.answer(Done::Locked, Ok(()))
    }

    fn power_off(&self, request: PowerRequest, force: bool) -> Result<(), String> {
        self.answer(Done::Power(request, force), Ok(()))
    }

    fn suspend(&self, hibernate: bool) -> Result<(), String> {
        self.answer(Done::Suspended(hibernate), Ok(()))
    }

    fn displays_off(&self) -> Result<(), String> {
        self.answer(Done::DisplaysOff, Ok(()))
    }

    fn screen_saver(&self) -> Result<(), String> {
        self.answer(Done::ScreenSaver, Ok(()))
    }

    fn volume(&self) -> Result<Volume, String> {
        match self.failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(*self.volume.lock().unwrap()),
        }
    }

    fn set_volume(&self, to: Volume) -> Result<Volume, String> {
        self.answer(Done::SetVolume(to), Ok(()))?;
        *self.volume.lock().unwrap() = to;
        Ok(to)
    }

    fn microphones(&self) -> Result<Vec<Microphone>, String> {
        match self.failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(self.microphones.lock().unwrap().clone()),
        }
    }

    fn set_microphone_mute(&self, id: &str, muted: bool) -> Result<(), String> {
        if self.vanished.lock().unwrap().iter().any(|gone| gone == id) {
            return Err(GONE.into());
        }
        self.answer(Done::SetMicrophoneMute(id.into(), muted), Ok(()))?;
        if let Some(device) = self
            .microphones
            .lock()
            .unwrap()
            .iter_mut()
            .find(|device| device.id == id)
        {
            device.muted = muted;
        }
        Ok(())
    }

    fn recycle_bin(&self) -> Result<RecycleBin, String> {
        match self.failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(*self.bin.lock().unwrap()),
        }
    }

    fn open_recycle_bin(&self) -> Result<(), String> {
        self.answer(Done::OpenedBin, Ok(()))
    }

    fn empty_recycle_bin(&self) -> Result<(), String> {
        self.answer(Done::EmptiedBin, Ok(()))?;
        *self.bin.lock().unwrap() = RecycleBin::default();
        Ok(())
    }

    fn appearance(&self) -> Result<Appearance, String> {
        match self.failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(*self.appearance.lock().unwrap()),
        }
    }

    fn set_appearance(&self, to: Appearance) -> Result<Appearance, String> {
        self.answer(Done::SetAppearance(to), Ok(()))?;
        *self.appearance.lock().unwrap() = to;
        Ok(to)
    }

    fn hdr_displays(&self) -> Result<Vec<HdrDisplay>, String> {
        match self.failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(self.hdr.lock().unwrap().clone()),
        }
    }

    fn set_hdr(&self, id: &str, hdr: bool) -> Result<(), String> {
        self.answer(Done::SetHdr(id.into(), hdr), Ok(()))?;
        if let Some(display) = self
            .hdr
            .lock()
            .unwrap()
            .iter_mut()
            .find(|display| display.id == id)
        {
            display.hdr = hdr;
        }
        Ok(())
    }

    fn show_desktop(&self) -> Result<(), String> {
        self.answer(Done::ShowedDesktop, Ok(()))
    }

    fn hidden_files(&self) -> Result<bool, String> {
        match self.failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(*self.hidden_files.lock().unwrap()),
        }
    }

    fn set_hidden_files(&self, shown: bool) -> Result<(), String> {
        self.answer(Done::SetHiddenFiles(shown), Ok(()))?;
        *self.hidden_files.lock().unwrap() = shown;
        Ok(())
    }

    fn removable_drives(&self) -> Result<Vec<Drive>, String> {
        match self.failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(self.drives.lock().unwrap().clone()),
        }
    }

    fn eject_drive(&self, id: &str) -> Result<(), String> {
        if self.busy.lock().unwrap().iter().any(|busy| busy == id) {
            return Err(BUSY.into());
        }
        self.answer(Done::Ejected(id.into()), Ok(()))?;
        self.drives.lock().unwrap().retain(|drive| drive.id != id);
        Ok(())
    }

    fn bluetooth_radios(&self) -> Result<Vec<BluetoothRadio>, String> {
        match self.failure.lock().unwrap().clone() {
            Some(why) => Err(why),
            None => Ok(self.bluetooth.lock().unwrap().clone()),
        }
    }

    fn set_bluetooth(&self, id: &str, on: bool) -> Result<(), String> {
        if self
            .gone_radios
            .lock()
            .unwrap()
            .iter()
            .any(|gone| gone == id)
        {
            return Err(RADIO_GONE.into());
        }
        self.answer(Done::SetBluetooth(id.into(), on), Ok(()))?;
        if let Some(radio) = self
            .bluetooth
            .lock()
            .unwrap()
            .iter_mut()
            .find(|radio| radio.id == id)
        {
            radio.on = on;
        }
        Ok(())
    }
}
