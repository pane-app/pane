//! Pane's crash record in the launcher (#133): after a run ended
//! unexpectedly, root search lists one root result, "Pane quit unexpectedly
//! last time", whose action opens the logs folder with the system's file
//! manager — the way the application update's row works (#54), a row in
//! root search and a word on the status line, with no new UI element.
//! Settings' About page shows the same notice ([`Launcher::log_notice`]).
//!
//! The notice goes when the user dismisses it (the row's Actions panel
//! entry, [`Launcher::dismiss_crash_notice`]), opens the folder, or Pane
//! next quits cleanly and starts again: it is only ever made at start, from
//! the markers `crate::diagnostics` found. A clean quit
//! ([`Launcher::quit_cleanly`]) removes this run's marker.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use super::{Entry, Launcher, Row, Screen, Status, off_thread};
use crate::diagnostics::{CRASH_NOTICE as NOTICE, CrashRecord};

/// The id of root search's row telling that Pane quit unexpectedly last
/// time.
pub const UNEXPECTED_QUIT: &str = "pane.unexpected-quit";

/// This run's crash record, and whether its notice is still shown.
#[derive(Default)]
pub(in crate::launcher) struct Notice {
    record: Option<Arc<CrashRecord>>,
    shown: bool,
}

/// What Pane knows of its log, for Settings' About page: where it is, and
/// whether the notice that Pane quit unexpectedly last time still shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogNotice {
    /// The logs folder.
    pub folder: PathBuf,
    /// Whether the run before this one ended unexpectedly, and the user
    /// has not dismissed the notice or opened the folder since.
    pub quit_unexpectedly: bool,
}

impl Notice {
    /// The row root search lists while the notice shows.
    pub(in crate::launcher) fn rows(&self) -> Vec<(Row, Entry)> {
        if !self.shown {
            return Vec::new();
        }
        vec![(
            Row {
                id: UNEXPECTED_QUIT.into(),
                title: NOTICE.into(),
                subtitle: Some("Open Pane's log folder; the log stays on this computer".into()),
                unavailable: None,
            },
            Entry::OpenLogFolder,
        )]
    }
}

impl Launcher {
    /// This launcher keeping `record`, this run's crash record
    /// ([`crate::diagnostics::start`]): when the run before ended
    /// unexpectedly, root search lists the notice and the status line says
    /// so; a clean quit removes the record's marker.
    pub fn with_crash_record(self, record: Arc<CrashRecord>) -> Self {
        {
            let mut state = self.lock();
            let shown = record.ended_unexpectedly();
            state.crash = Notice {
                record: Some(record),
                shown,
            };
            if shown
                && matches!(state.view.screen, Screen::Root { .. })
                && state.view.status == Status::Idle
            {
                state.view.status = Status::Result(NOTICE.into());
            }
            self.refresh(&mut state);
        }
        self
    }

    /// Where Pane's log is and whether the notice still shows, for a second
    /// surface of it beside root search (Settings' About page); `None` when
    /// this launcher was given no crash record.
    pub fn log_notice(&self) -> Option<LogNotice> {
        let state = self.lock();
        let record = state.crash.record.as_ref()?;
        Some(LogNotice {
            folder: record.folder().to_path_buf(),
            quit_unexpectedly: state.crash.shown,
        })
    }

    /// Takes the notice away, as the user chose: root search no longer
    /// lists it, nor does the About page show it.
    pub fn dismiss_crash_notice(&self) {
        let mut state = self.lock();
        if std::mem::take(&mut state.crash.shown) {
            self.refresh(&mut state);
        }
        drop(state);
        self.changed();
    }

    /// Opens the logs folder with the system's file manager, off the
    /// calling thread, and says whether it opened; the notice goes, as
    /// opening the folder is what it asked of the user. Await the returned
    /// future to apply the result.
    pub fn open_log_folder(&self) -> impl Future<Output = ()> + Send + 'static {
        let launcher = self.clone();
        async move { launcher.open_logs().await }
    }

    async fn open_logs(&self) {
        let (system, folder) = {
            let mut state = self.lock();
            let Some(folder) = state
                .crash
                .record
                .as_ref()
                .map(|record| record.folder().to_path_buf())
            else {
                return;
            };
            if std::mem::take(&mut state.crash.shown) {
                self.refresh(&mut state);
            }
            (state.system.clone(), folder)
        };
        let target = folder.to_string_lossy().into_owned();
        let open = move || crate::system::System::open(system.as_ref(), &target, None);
        let opened = off_thread(open).await;
        self.show(match opened {
            Ok(()) => Status::Result("Opened Pane's log folder".into()),
            Err(why) => Status::Error(format!("Could not open Pane's log folder: {why}")),
        });
    }

    /// Records that Pane quits cleanly: this run's marker goes, so the next
    /// start reports nothing. The tray's or menu bar's Quit, closing the
    /// launcher's window and the system ending the session all come here;
    /// calling it again does nothing.
    pub fn quit_cleanly(&self) {
        let record = self.lock().crash.record.clone();
        if let Some(record) = record {
            record.clean_quit();
        }
    }
}
