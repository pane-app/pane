//! The provider the user chose for each capability (ADR 0041, #154):
//! Pane's own record, `capability-choices.json` beside
//! `installed.json` (see `choices`), never extension data. For each
//! capability at its major it holds the chosen provider's identity key.
//! It is kept across restarts, and across a reload or an update of the
//! chosen provider, which keep its identity; it is forgotten when the
//! chosen provider is uninstalled, so a stale choice never lingers. Until
//! the user chooses, the first provider in install order serves and a
//! later install changes nothing (see `operations`), and a capability
//! with one provider needs no choice.
//!
//! While the chosen provider cannot serve — it is disabled, paused,
//! missing or waiting — calls fall back to the next provider that can, in
//! the default order, and return to the chosen one when it can serve
//! again; consumers wait only when no provider can serve, which a later
//! slice delivers. A change applies to the next call, without reloading
//! or restarting any consumer, as the operation router reads the choice
//! per call. The record is written atomically, one write at a time; a
//! write that cannot happen puts the choices in Pane back to what was
//! last recorded, unless the chosen provider was uninstalled meanwhile.
//! An unreadable record is never overwritten.
//!
//! Also here: what Settings' Capabilities section reads
//! ([`Launcher::capabilities`]) — the capabilities two or more installed
//! packages provide, their providers and consumers, and why a choice
//! falls back while the chosen provider cannot serve.

use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

use super::{Launcher, State, off_thread};
use crate::atomic::{Readers, write_atomically};
use crate::operations::Installed;
use crate::packages::{InstalledPackage, PackageIdentity};

/// The record's file name, beside `installed.json`.
const FILE: &str = "capability-choices.json";
/// The record's version; a record of another version is not read.
const VERSION: u64 = 1;

/// The chosen providers, as Pane records them: for each capability, the
/// identity key of the provider the user chose. Until one is chosen, the
/// default order — install order — decides (see `operations`).
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub(crate) struct CapabilityChoices {
    by_capability: BTreeMap<String, String>,
}

impl CapabilityChoices {
    /// The identity key of the provider the user chose for `capability`,
    /// if they chose one.
    pub(crate) fn provider_of(&self, capability: &str) -> Option<&str> {
        self.by_capability.get(capability).map(String::as_str)
    }
}

/// The recorded choices, and where they are recorded.
#[derive(Default)]
pub(super) struct Kept {
    /// Where they are recorded; `None` for a launcher that installs no
    /// packages.
    file: Option<PathBuf>,
    /// The choices as they are in Pane.
    pub(super) chosen: CapabilityChoices,
    /// Why the record could not be read, if it could not; it is then never
    /// overwritten.
    unreadable: Option<String>,
    /// The choices as last recorded (or read). Held while the record is
    /// written, so writes happen one at a time; see
    /// [`Launcher::save_capability_choices`].
    recorded: Arc<Mutex<CapabilityChoices>>,
    /// The identity keys whose choices were forgotten when they were
    /// uninstalled; a failed write does not bring them back.
    forgotten: HashSet<String>,
}

impl Kept {
    /// Reads the choices recorded in `dir`.
    pub(super) fn open(dir: &Path) -> Kept {
        let file = dir.join(FILE);
        let mut record = Kept {
            file: Some(file.clone()),
            ..Kept::default()
        };
        let read = match std::fs::read_to_string(&file) {
            Ok(text) => match serde_json::from_str::<Map<String, Value>>(&text) {
                Ok(fields) => match fields.get("version").and_then(Value::as_u64) {
                    Some(version) if version == VERSION => read(&fields)
                        .map(Some)
                        .map_err(|error| format!("{} is invalid: {error}", file.display())),
                    Some(version) => Err(format!(
                        "{} has version {version}, which this Pane does not read",
                        file.display()
                    )),
                    None => Err(format!("{} is invalid: it has no version", file.display())),
                },
                Err(error) => Err(format!("{} is invalid: {error}", file.display())),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{} cannot be read: {error}", file.display())),
        };
        match read {
            Ok(chosen) => record.chosen = chosen.unwrap_or_default(),
            Err(problem) => record.unreadable = Some(problem),
        }
        *record.recorded.lock().unwrap_or_else(|p| p.into_inner()) = record.chosen.clone();
        record
    }

    /// The record's text as the choices are now, and where it goes; `Err`
    /// if there is nowhere to write it.
    fn text(&self) -> Result<(PathBuf, String), String> {
        if let Some(problem) = &self.unreadable {
            return Err(format!("Pane does not replace it: {problem}"));
        }
        let file = self.file.clone().ok_or_else(|| {
            "this launcher installs no packages, so it keeps no provider choices".to_string()
        })?;
        let mut fields = Map::new();
        fields.insert("version".into(), VERSION.into());
        let mut chosen = Map::new();
        for (capability, provider) in &self.chosen.by_capability {
            chosen.insert(capability.clone(), Value::String(provider.clone()));
        }
        fields.insert("chosen".into(), Value::Object(chosen));
        let text =
            serde_json::to_string_pretty(&Value::Object(fields)).map_err(|e| e.to_string())?;
        Ok((file, text))
    }

    /// Forgets the choices whose chosen provider is the package with
    /// `identity`, which was uninstalled: exactly its choices, not those
    /// of a package whose identity only starts the same. Whether any went.
    pub(super) fn forget(&mut self, identity: &PackageIdentity) -> bool {
        let key = identity.key();
        self.forgotten.insert(key.clone());
        let before = self.chosen.by_capability.len();
        self.chosen
            .by_capability
            .retain(|_, provider| *provider != key);
        self.chosen.by_capability.len() != before
    }
}

/// The choices in a record's fields (besides `version`).
fn read(fields: &Map<String, Value>) -> Result<CapabilityChoices, String> {
    let mut choices = CapabilityChoices::default();
    let Some(chosen) = fields.get("chosen") else {
        return Ok(choices);
    };
    let chosen = chosen.as_object().ok_or("`chosen` is not an object")?;
    for (capability, provider) in chosen {
        let Some(provider) = provider.as_str() else {
            return Err(format!("the choice for `{capability}` is not a source"));
        };
        choices
            .by_capability
            .insert(capability.clone(), provider.to_owned());
    }
    Ok(choices)
}

/// One capability the Settings window's Capabilities section lists (#154):
/// one with two or more installed providers, so the user has a choice to
/// make. A capability with one provider needs no choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    /// The capability's name, as its providers declare it.
    pub name: String,
    /// Its installed providers, in install order.
    pub providers: Vec<CapabilityProvider>,
    /// The identity key of the provider Pane routes the next call to: the
    /// user's choice, else the first provider in install order.
    pub selected: String,
    /// The titles of the installed packages that use the capability.
    pub consumers: Vec<String>,
    /// "<chosen> is disabled; using <other>" (or paused, waiting) while the
    /// user's choice falls back to another provider; `None` while the
    /// chosen provider serves, and when no choice was made.
    pub fallback: Option<String>,
}

/// One provider of a capability, as the Settings Capabilities section
/// offers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityProvider {
    /// The provider's identity key, what a choice commits.
    pub source: String,
    /// The provider's display title.
    pub title: String,
}

impl Launcher {
    /// The capabilities two or more installed packages provide, in name
    /// order, for the Settings Capabilities section: their providers, the
    /// provider Pane routes the next call to, the consumers that use each
    /// and why a choice falls back while the chosen provider cannot serve.
    /// Read now, as the operation router resolves calls now.
    pub fn capabilities(&self) -> Vec<Capability> {
        let state = self.lock();
        let data = self
            .installation
            .as_ref()
            .map(|installation| installation.data.clone());
        let installed = super::installed_of(&state, data);
        // Each capability's providers, in install order.
        let mut providers: BTreeMap<String, Vec<&InstalledPackage>> = BTreeMap::new();
        for package in &installed.packages {
            let Ok(manifest) = &package.manifest else {
                continue;
            };
            for provides in &manifest.provides {
                providers
                    .entry(provides.capability.clone())
                    .or_default()
                    .push(package);
            }
        }
        providers
            .into_iter()
            .filter(|(_, providers)| providers.len() > 1)
            .map(|(capability, providers)| {
                // The consumers that use it: an installed package's
                // manifest says, whatever its state.
                let consumers: Vec<String> = installed
                    .packages
                    .iter()
                    .filter(|package| {
                        package.manifest.as_ref().is_ok_and(|manifest| {
                            manifest
                                .uses
                                .iter()
                                .any(|used| used.capability == capability)
                        })
                    })
                    .map(InstalledPackage::title)
                    .collect();
                let selected = installed
                    .chosen
                    .provider_of(&capability)
                    .filter(|chosen| {
                        providers
                            .iter()
                            .any(|package| package.identity.key() == *chosen)
                    })
                    .map_or_else(|| providers[0].identity.key(), ToOwned::to_owned);
                let fallback = fallback_note(&installed, &capability, &selected);
                Capability {
                    name: capability,
                    providers: providers
                        .iter()
                        .map(|package| CapabilityProvider {
                            source: package.identity.key(),
                            title: package.title(),
                        })
                        .collect(),
                    selected,
                    consumers,
                    fallback,
                }
            })
            .collect()
    }

    /// Records `provider` — the identity key of an installed package that
    /// provides `capability` — as the provider the user chose for it: a
    /// choice of Pane's own, kept across restarts, updates and reloads and
    /// forgotten when the chosen provider is uninstalled. Every consumer's
    /// next call goes to it, without reloading or restarting anything.
    /// Refused, with the reason, when no installed package with that key
    /// provides the capability. Await the returned future for whether the
    /// choice was written; a write that could not happen puts it back to
    /// what was last recorded.
    pub fn choose_provider(
        &self,
        capability: &str,
        provider: &str,
    ) -> Result<impl Future<Output = Result<(), String>> + Send + 'static, String> {
        let mut state = self.lock();
        let data = self
            .installation
            .as_ref()
            .map(|installation| installation.data.clone());
        let installed = super::installed_of(&state, data);
        let offers: Vec<&InstalledPackage> = installed
            .packages
            .iter()
            .filter(|package| {
                package.manifest.as_ref().is_ok_and(|manifest| {
                    manifest
                        .provides
                        .iter()
                        .any(|provides| provides.capability == capability)
                })
            })
            .collect();
        if !offers
            .iter()
            .any(|package| package.identity.key() == provider)
        {
            let names: Vec<String> = offers.iter().map(|package| package.title()).collect();
            let offering = match names.as_slice() {
                [] => format!("no installed extension provides `{capability}`"),
                names => format!(
                    "the installed extensions that provide it are {}",
                    crate::platform::join(names)
                ),
            };
            return Err(format!(
                "`{provider}` does not provide `{capability}`: {offering}"
            ));
        }
        state
            .capability_choices
            .chosen
            .by_capability
            .insert(capability.to_owned(), provider.to_owned());
        self.changed();
        let saving = self.clone();
        Ok(async move { off_thread(move || saving.save_capability_choices()).await })
    }

    /// Writes the chosen providers as they are when the write begins,
    /// blocking: run it off the window's thread. If the record cannot be
    /// written, the choices in Pane go back to what was last recorded,
    /// unless the provider was uninstalled since: its choice never comes
    /// back.
    fn save_capability_choices(&self) -> Result<(), String> {
        let recorded = self.lock().capability_choices.recorded.clone();
        let mut recorded = recorded.lock().unwrap_or_else(|p| p.into_inner());
        let (text, chosen) = {
            let state = self.lock();
            let record = &state.capability_choices;
            (record.text(), record.chosen.clone())
        };
        let saved = text.and_then(|(file, text)| {
            write_atomically(&file, text.as_bytes(), Readers::Default).map_err(|e| e.to_string())
        });
        if saved.is_ok() {
            *recorded = chosen;
        } else {
            let mut state = self.lock();
            let record = &mut state.capability_choices;
            // What was last recorded is back in Pane, except a provider
            // uninstalled since: its choice never comes back.
            record.chosen = recorded.clone();
            record
                .chosen
                .by_capability
                .retain(|_, provider| !record.forgotten.contains(provider));
        }
        saved
    }

    /// Forgets the choices whose chosen provider is the uninstalled
    /// package with `identity`. Returns what writes the record without
    /// them, to run off the window's thread; nothing to write if it was
    /// chosen for none.
    pub(super) fn forget_capability_choices_of(
        &self,
        state: &mut State,
        identity: &PackageIdentity,
    ) -> Option<impl FnOnce() -> Result<(), String> + Send + 'static> {
        if !state.capability_choices.forget(identity) {
            return None;
        }
        let launcher = self.clone();
        Some(move || launcher.save_capability_choices())
    }
}

/// The Capabilities section's fallback note for `capability`, whose next
/// call goes to `selected` (the chosen provider, else the first
/// installed): why the chosen provider cannot serve and who serves
/// instead, or `None` while it serves or no choice was made.
fn fallback_note(installed: &Installed, capability: &str, selected: &str) -> Option<String> {
    let chosen = installed
        .packages
        .iter()
        .find(|package| package.identity.key() == selected)?;
    let entry = chosen
        .manifest
        .as_ref()
        .ok()?
        .provides
        .iter()
        .find(|provides| provides.capability == capability)?;
    let why = installed.cannot_serve_note(chosen, entry)?;
    // Who serves instead: the first provider that can, in the order Pane
    // calls them — the chosen one first, which cannot, so the first in
    // install order that can.
    let other = installed
        .call_order(None, capability, None)
        .into_iter()
        .find(|(package, entry)| installed.cannot_serve_note(package, entry).is_none())
        .map(|(package, _)| package.title());
    Some(match other {
        Some(title) => format!("{why}; using {title}"),
        None => format!("{why}, and no other provider can serve it now"),
    })
}

/// The record's text is read back by `read`: a round trip.
#[test]
fn the_choices_round_trip_through_their_record() {
    let mut choices = CapabilityChoices::default();
    choices
        .by_capability
        .insert("acme:translate@1".into(), "local:/a".into());
    let mut fields = Map::new();
    fields.insert("version".into(), VERSION.into());
    let mut chosen = Map::new();
    chosen.insert(
        "acme:translate@1".to_owned(),
        Value::String("local:/a".into()),
    );
    fields.insert("chosen".into(), Value::Object(chosen));
    assert_eq!(read(&fields).unwrap(), choices);
    assert_eq!(choices.provider_of("acme:translate@1"), Some("local:/a"));
    assert_eq!(choices.provider_of("acme:notes@1"), None);
}

/// An unreadable field refuses the record rather than being ignored.
#[test]
fn a_choice_that_is_not_a_source_is_refused() {
    let mut fields = Map::new();
    fields.insert("version".into(), VERSION.into());
    let mut chosen = Map::new();
    chosen.insert("acme:translate@1".to_owned(), Value::Bool(true));
    fields.insert("chosen".into(), Value::Object(chosen));
    let error = read(&fields).unwrap_err();
    assert_eq!(error, "the choice for `acme:translate@1` is not a source");
}

/// Forgetting a provider removes exactly its choices.
#[test]
fn forgetting_removes_the_choices_of_the_uninstalled_provider() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (
        PackageIdentity::local(a.path()).unwrap(),
        PackageIdentity::local(b.path()).unwrap(),
    );
    let mut record = Kept {
        file: None,
        forgotten: HashSet::new(),
        unreadable: None,
        recorded: Arc::default(),
        chosen: CapabilityChoices::default(),
    };
    record
        .chosen
        .by_capability
        .insert("acme:translate@1".into(), a.key());
    record
        .chosen
        .by_capability
        .insert("acme:notes@1".into(), b.key());
    assert!(record.forget(&a));
    assert_eq!(record.chosen.provider_of("acme:translate@1"), None);
    assert_eq!(
        record.chosen.provider_of("acme:notes@1"),
        Some(b.key().as_str())
    );
    // Nothing more to forget.
    assert!(!record.forget(&a));
}

/// A package whose identity only starts the same is not forgotten.
#[test]
fn forgetting_matches_the_whole_identity() {
    let folders = tempfile::tempdir().unwrap();
    let a = folders.path().join("a");
    let ab = folders.path().join("ab");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&ab).unwrap();
    let (a, ab) = (
        PackageIdentity::local(&a).unwrap(),
        PackageIdentity::local(&ab).unwrap(),
    );
    let mut record = Kept {
        file: None,
        forgotten: HashSet::new(),
        unreadable: None,
        recorded: Arc::default(),
        chosen: CapabilityChoices::default(),
    };
    record
        .chosen
        .by_capability
        .insert("acme:translate@1".into(), ab.key());
    assert!(!record.forget(&a));
    assert_eq!(
        record.chosen.provider_of("acme:translate@1"),
        Some(ab.key().as_str())
    );
}
