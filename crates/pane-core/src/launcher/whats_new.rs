//! What's New in the launcher (ADR 0050, #505): after an update of Pane
//! (see `crate::whats_new`, which finds one at start), root search lists
//! **What's New in Pane <version>** for a day — for a blank query near the
//! top, under the Update section, below Restart to Update Pane when both
//! are listed, and in compact mode too. It goes once the day is over, when
//! the user opens it, or when they dismiss it from the Actions panel
//! ([`Launcher::dismiss_notice`]); the record in the data folder keeps that
//! across restarts.
//!
//! Enter shows the release's notes, the GitHub release's body, as Markdown
//! in a detail view, whose primary action opens the release's page. The
//! notes come from the listing Pane's own update check kept, or else from
//! one request for the release alone; if neither has them, Enter opens the
//! release's page instead. The view is one of Pane's own designed trees
//! (`Screen::PaneForm`): the Markdown, drawn by the renderer every
//! extension's Markdown is drawn by, with the page's button under it. The
//! Markdown parser reads no HTML, and the tree's images are never loaded
//! for it, so release notes cannot reach anything beyond their text.

use std::path::{Path, PathBuf};

use super::pane_form::{self, PaneForm};
use super::{Entry, FormPurpose, Launcher, Row, State, Status, next_form_id, off_thread};
use crate::runtime::{Markdown, Node, NodeKind};
use crate::whats_new::{RECORD, Record, Started};

/// The id of root search's What's New row.
pub const WHATS_NEW_ROW: &str = "pane.whats-new";

/// The Actions panel's entry that dismisses What's New.
pub const DISMISS_WHATS_NEW: &str = "Dismiss What's New";

/// The detail view's primary action, which opens the release's page.
const OPEN_PAGE: &str = "Open Release Page";

/// What's New as this run lists it: the record it is kept in, and the
/// version of Pane this run is.
#[derive(Default)]
pub(in crate::launcher) struct WhatsNew {
    record: Record,
    path: Option<PathBuf>,
    version: Option<String>,
}

impl WhatsNew {
    /// The version What's New is listed for at `now`, if it is.
    fn listed(&self, now: u64) -> Option<&str> {
        let shown = self.record.shown.as_ref()?;
        (Some(shown.version.as_str()) == self.version.as_deref() && shown.listed_at(now))
            .then_some(shown.version.as_str())
    }

    /// The row root search lists while What's New is listed at `now`.
    pub(in crate::launcher) fn rows(&self, now: u64) -> Vec<(Row, Entry)> {
        let Some(version) = self.listed(now) else {
            return Vec::new();
        };
        vec![(
            Row {
                id: WHATS_NEW_ROW.into(),
                title: format!("What's New in Pane {version}"),
                subtitle: Some("See what changed in this version".into()),
                unavailable: None,
            },
            Entry::WhatsNew,
        )]
    }

    /// Whether `rows`, the rows on screen, list What's New although its
    /// day ended by `now`: the list is to be made again.
    pub(in crate::launcher) fn expired_in_view(&self, rows: &[Row], now: u64) -> bool {
        self.listed(now).is_none() && rows.iter().any(|row| row.id == WHATS_NEW_ROW)
    }

    /// Takes What's New away for good: opened or dismissed. Whether it was
    /// listed.
    fn clear(&mut self) -> bool {
        if self.record.shown.take().is_none() {
            return false;
        }
        if let Some(path) = &self.path {
            self.record.save(path);
        }
        true
    }

    /// The version of Pane this run is, as What's New was given it: for
    /// the toast that says Pane was updated, when no updater names it.
    pub(in crate::launcher) fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

/// What opening What's New found to show.
enum Notes {
    /// The release's notes, as Markdown, and its page.
    Found { notes: String, page: String },
    /// No notes to show: the release's page opens instead.
    Page(String),
}

impl Launcher {
    /// This launcher listing What's New as the record in the data folder
    /// `data` says, for version `version` of Pane, which this start found
    /// to be `started` ([`crate::whats_new::start`]): a start on a newer
    /// version also says "Updated to Pane <version>" the first time the
    /// launcher is shown.
    pub fn with_whats_new(self, data: &Path, version: &str, started: &Started) -> Self {
        {
            let mut state = self.lock();
            let path = data.join(RECORD);
            state.whats_new = WhatsNew {
                record: Record::open(&path).unwrap_or_default(),
                path: Some(path),
                version: Some(version.to_owned()),
            };
            if let Started::Updated { from } = started {
                state
                    .updates
                    .note_updated(from.as_deref().unwrap_or_default());
            }
            self.refresh(&mut state);
        }
        self
    }

    /// Takes away the notice root search's row `id` is, as the user chose
    /// from the Actions panel: that Pane quit unexpectedly last time, or
    /// What's New. Any other row is left as it is.
    pub fn dismiss_notice(&self, id: &str) {
        if id == super::UNEXPECTED_QUIT {
            self.dismiss_crash_notice();
            return;
        }
        if id != WHATS_NEW_ROW {
            return;
        }
        let mut state = self.lock();
        if state.whats_new.clear() {
            self.refresh(&mut state);
        }
        drop(state);
        self.changed();
    }

    /// Opens What's New, which the user activated: it is no longer
    /// listed, and its notes show in a detail view — read from the listing
    /// Pane's update check kept, or else asked for once — or, without them,
    /// the release's page opens.
    pub(in crate::launcher) async fn open_whats_new(&self, epoch: u64) {
        let (version, kept) = {
            let mut state = self.lock();
            let now = state.clock.now();
            let Some(version) = state.whats_new.listed(now).map(str::to_owned) else {
                state.view.status = Status::Idle;
                return;
            };
            // Opened: it has done what it is for, and goes.
            state.whats_new.clear();
            self.refresh(&mut state);
            let kept = state.updates.kept_release(&version);
            (version, kept)
        };
        self.changed();
        let source = self
            .application
            .as_ref()
            .map(|application| application.source().clone());
        let page_of = |page: Option<String>| {
            page.or_else(|| source.as_ref().map(|source| source.release_page(&version)))
                .unwrap_or_else(|| {
                    crate::releases::ReleaseSource::published().release_page(&version)
                })
        };
        let notes = match kept {
            Some(release) if release.notes.is_some() => Notes::Found {
                notes: release.notes.unwrap_or_default(),
                page: page_of(release.page),
            },
            kept => {
                let read = match &source {
                    Some(source) => {
                        let (source, asked) = (source.clone(), version.clone());
                        off_thread(move || source.release(&asked)).await
                    }
                    None => Err("this Pane reads no release source".into()),
                };
                match read {
                    Ok(release) => match release.notes {
                        Some(notes) => Notes::Found {
                            notes,
                            page: page_of(release.page),
                        },
                        None => Notes::Page(page_of(release.page)),
                    },
                    Err(why) => {
                        crate::diagnostic!(
                            "The notes of Pane {version} could not be read, so its release page \
                             opens instead: {why}"
                        );
                        Notes::Page(page_of(kept.and_then(|release| release.page)))
                    }
                }
            }
        };
        match notes {
            Notes::Found { notes, page } => {
                let Some(mut state) = self.lock_if_current(epoch) else {
                    return;
                };
                state.view.status = Status::Idle;
                show_notes(&mut state, &version, &notes, page);
                drop(state);
                self.changed();
            }
            Notes::Page(page) => self.open_url(epoch, page).await,
        }
    }
}

/// Shows the notes of Pane `version`, Markdown, in a detail view over root
/// search, its primary action opening `page`: Back returns to root search.
fn show_notes(state: &mut State, version: &str, notes: &str, page: String) {
    let title = format!("What's New in Pane {version}");
    let markdown = Node {
        kind: NodeKind::Markdown(Markdown {
            blocks: crate::markdown::parse(notes),
        }),
        key: Some("notes".into()),
        ..Node::plain()
    };
    let form = PaneForm::of(
        next_form_id(),
        &title,
        OPEN_PAGE,
        Vec::new(),
        Some(markdown),
    );
    pane_form::open_pane_form(state, FormPurpose::ReleaseNotes(page), form, title);
}
