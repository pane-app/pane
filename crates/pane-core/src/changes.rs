//! Telling the window that the launcher changed in the background, such as
//! a package being developed that was built or reloaded, so that it
//! redraws although no user action is waiting for an answer.

/// A channel from the launcher's background work to the window.
pub fn channel() -> (ChangeSender, Changes) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    (ChangeSender(sender), Changes(receiver))
}

/// Tells the window that the launcher changed in the background.
#[derive(Clone, Debug)]
pub struct ChangeSender(tokio::sync::mpsc::UnboundedSender<()>);

impl ChangeSender {
    pub(crate) fn changed(&self) {
        let _ = self.0.send(());
    }
}

/// The window's end of [`channel`].
#[derive(Debug)]
pub struct Changes(tokio::sync::mpsc::UnboundedReceiver<()>);

impl Changes {
    /// Resolves when the launcher changed since the last call, or with
    /// `None` once it has stopped. Several changes meanwhile are one.
    pub async fn next(&mut self) -> Option<()> {
        let change = self.0.recv().await;
        while self.0.try_recv().is_ok() {}
        change
    }
}
