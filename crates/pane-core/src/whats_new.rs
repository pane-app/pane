//! What's New after an update of Pane (ADR 0050, #505): each start of Pane
//! records the version it runs in its data folder (`whats-new.json`), and a
//! start on a newer version than the last one is an update. That covers a
//! restart into an update (`--updated-from`) and an update the setup or a
//! package installed by hand, which no running Pane saw. A data folder with
//! Pane's data in it but no record belongs to a Pane from before this
//! record, so its first start counts as an update too; a fresh data folder
//! is a first start, not an update.
//!
//! An update starts **What's New in Pane <version>** in root search (see
//! the launcher's `whats_new`): the record keeps since when, so it stays
//! listed across restarts for [`SHOWN_FOR`], until the user opens it or
//! dismisses it, which clears it from the record.
//!
//! The record is read and written at start by [`start`], before anything
//! else of this run writes to the data folder, so that the data a Pane
//! from before left there is told apart from what this run makes.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::atomic::{Readers, write_atomically};

/// The record's file in Pane's data folder.
pub const RECORD: &str = "whats-new.json";

/// How long What's New stays listed after the start that found the update,
/// in milliseconds: a day, as Raycast's "See What's New" stays.
pub const SHOWN_FOR: u64 = 24 * 3600 * 1000;

/// What the record holds: the version the last start ran, and the What's
/// New that is listed, if one is. Times are the launcher clock's, in
/// milliseconds since the Unix epoch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    version: u32,
    /// The version of Pane the last start ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_run: Option<String>,
    /// What's New, while it is listed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) shown: Option<Shown>,
}

/// What's New for one version, listed since `since`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Shown {
    pub(crate) version: String,
    pub(crate) since: u64,
}

impl Shown {
    /// Whether it is still listed at `now`: less than [`SHOWN_FOR`] after
    /// the start that found the update. A clock set back before that start
    /// keeps it, as it cannot tell how long it was listed.
    pub(crate) fn listed_at(&self, now: u64) -> bool {
        now < self.since.saturating_add(SHOWN_FOR)
    }
}

/// What a start of Pane found, by its record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Started {
    /// The first start on this data folder: no update, nothing listed.
    Fresh,
    /// A start of the version that ran last.
    Again,
    /// A start on a newer version than the last start's: `from`, when it
    /// is known (a data folder from before the record knows none). What's
    /// New is listed, and the launcher says "Updated to Pane <version>"
    /// the first time it is shown.
    Updated { from: Option<String> },
}

impl Record {
    /// The version of the record's format this Pane writes.
    const VERSION: u32 = 1;

    /// The record at `path`; `None` when there is none, or none this Pane
    /// can read.
    pub(crate) fn open(path: &Path) -> Option<Record> {
        fs::read(path)
            .ok()
            .and_then(|text| serde_json::from_slice::<Record>(&text).ok())
            .filter(|record| record.version == Record::VERSION)
    }

    /// Writes the record to `path`, replacing it whole. One that cannot be
    /// written is logged: What's New may then show again, or not at all,
    /// which is all it costs.
    pub(crate) fn save(&self, path: &Path) {
        let record = Record {
            version: Record::VERSION,
            ..self.clone()
        };
        let written = serde_json::to_vec_pretty(&record)
            .map_err(std::io::Error::other)
            .and_then(|text| {
                if let Some(folder) = path.parent() {
                    fs::create_dir_all(folder)?;
                }
                write_atomically(path, &text, Readers::Default)
            });
        if let Err(error) = written {
            crate::diagnostic!(
                "Pane's What's New record {} could not be written: {error}",
                path.display()
            );
        }
    }
}

/// Records that Pane `version` starts on the data folder `data`, at `now`
/// (milliseconds since the Unix epoch), and says whether it is an update:
/// `updated_from` names the version a restart into an update came from
/// (`--updated-from`), else the record's last version does, else a data
/// folder holding Pane's data counts as an update from an unknown version.
/// An update lists What's New for `version` from `now`. A start of an
/// older version than the last one (Pane put back by hand) lists nothing.
/// Call it at start, before anything else of the run writes to `data`.
pub fn start(data: &Path, version: &str, updated_from: Option<&str>, now: u64) -> Started {
    let path = data.join(RECORD);
    let record = Record::open(&path);
    let previous = updated_from
        .map(str::to_owned)
        .or_else(|| record.as_ref().and_then(|record| record.last_run.clone()));
    let started = match (&previous, &record) {
        (Some(from), _) if from == version => Started::Again,
        // A version Pane cannot compare is a different one: the notes of
        // the version it runs are what it can show.
        (Some(from), _) if crate::application_update::newer(version, from).unwrap_or(true) => {
            Started::Updated {
                from: Some(from.clone()),
            }
        }
        (Some(_), _) => Started::Again,
        (None, None) if holds_pane_data(data) => Started::Updated { from: None },
        (None, None) => Started::Fresh,
        // A record that names no last version: written by a start that
        // could not record one, so nothing to compare with.
        (None, Some(_)) => Started::Again,
    };
    let mut kept = record.clone().unwrap_or_default();
    kept.last_run = Some(version.to_owned());
    if matches!(started, Started::Updated { .. }) {
        kept.shown = Some(Shown {
            version: version.to_owned(),
            since: now,
        });
    }
    if record.as_ref() != Some(&kept) {
        kept.save(&path);
    }
    started
}

/// Whether the data folder `data` holds what a Pane before the record left
/// there: anything but the logs folder (which a test's or smoke's data
/// folder holds, and a crash record may have just made) and the record
/// itself.
fn holds_pane_data(data: &Path) -> bool {
    fs::read_dir(data).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name();
            name != "logs" && name != RECORD
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn shown(data: &Path) -> Option<Shown> {
        Record::open(&data.join(RECORD)).and_then(|record| record.shown)
    }

    #[test]
    fn a_fresh_data_folder_is_a_first_start_and_lists_nothing() {
        let data = tempfile::tempdir().unwrap();
        // A crash record's logs are not Pane's data.
        fs::create_dir(data.path().join("logs")).unwrap();
        assert_eq!(start(data.path(), "0.1.2", None, NOW), Started::Fresh);
        assert_eq!(shown(data.path()), None);
        // The next start of the same version is no update either.
        assert_eq!(start(data.path(), "0.1.2", None, NOW + 1), Started::Again);
        assert_eq!(shown(data.path()), None);
        // A folder that does not exist yet is fresh too.
        let missing = data.path().join("missing");
        assert_eq!(start(&missing, "0.1.2", None, NOW), Started::Fresh);
    }

    #[test]
    fn a_newer_version_than_the_last_start_lists_what_s_new_from_now() {
        let data = tempfile::tempdir().unwrap();
        start(data.path(), "0.1.2", None, NOW);
        // Installed by the setup: no `--updated-from`, the record tells.
        assert_eq!(
            start(data.path(), "0.1.3", None, NOW + 5),
            Started::Updated {
                from: Some("0.1.2".into())
            }
        );
        assert_eq!(
            shown(data.path()),
            Some(Shown {
                version: "0.1.3".into(),
                since: NOW + 5
            })
        );
        // A start after it keeps it listed, from the same time.
        assert_eq!(start(data.path(), "0.1.3", None, NOW + 9), Started::Again);
        assert_eq!(shown(data.path()).unwrap().since, NOW + 5);
    }

    #[test]
    fn a_restart_into_an_update_names_the_version_it_came_from() {
        let data = tempfile::tempdir().unwrap();
        start(data.path(), "0.1.2", None, NOW);
        assert_eq!(
            start(data.path(), "99.0.0", Some("0.1.2"), NOW),
            Started::Updated {
                from: Some("0.1.2".into())
            }
        );
        assert_eq!(shown(data.path()).unwrap().version, "99.0.0");
    }

    #[test]
    fn a_data_folder_from_before_the_record_counts_as_an_update() {
        let data = tempfile::tempdir().unwrap();
        fs::write(data.path().join("settings.json"), "{}").unwrap();
        assert_eq!(
            start(data.path(), "0.1.2", None, NOW),
            Started::Updated { from: None }
        );
        assert_eq!(shown(data.path()).unwrap().version, "0.1.2");
    }

    #[test]
    fn an_older_version_lists_nothing() {
        let data = tempfile::tempdir().unwrap();
        start(data.path(), "0.2.0", None, NOW);
        assert_eq!(start(data.path(), "0.1.9", None, NOW), Started::Again);
        assert_eq!(shown(data.path()), None);
    }

    #[test]
    fn what_s_new_is_listed_for_a_day() {
        let listed = Shown {
            version: "0.1.2".into(),
            since: NOW,
        };
        assert!(listed.listed_at(NOW));
        assert!(listed.listed_at(NOW + SHOWN_FOR - 1));
        assert!(!listed.listed_at(NOW + SHOWN_FOR));
    }
}
