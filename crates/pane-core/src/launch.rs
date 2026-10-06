//! The launch record: how a command was launched, and with what (ADR 0037).
//!
//! Every command receives one launch record on every way in, through its
//! view entry point (`render`) or its run entry point (`run`, a no-view
//! command's). It says whether the user launched the command or Pane did in
//! the background, where the launch came from (root search, an alias, a
//! fallback, a global hotkey, a quick slot, another command, a schedule),
//! the values of the command's arguments, the text sent to it through its
//! alias or as a fallback, and the JSON context another command passed.
//! This is the typed record of `pane:extension/commands` (wit/commands.wit);
//! the SDKs hand it to commands as a typed value.

/// Whether the user launched the command, or Pane did in the background.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LaunchType {
    /// The user launched it, or another command did as if the user had.
    #[default]
    UserInitiated,
    /// Pane ran it without a window: its schedule, or another command's
    /// background launch.
    Background,
}

/// Where a launch came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LaunchSource {
    /// Enter (or a click) on the command's row in root search.
    #[default]
    RootSearch,
    /// The command's alias, alone or followed by the fallback text.
    Alias,
    /// The command chosen as a fallback for the text typed.
    Fallback,
    /// The command's global hotkey.
    Hotkey,
    /// The quick slot the command is pinned to.
    QuickSlot,
    /// Another command, through `launch`.
    Command,
    /// The command's own schedule.
    Schedule,
}

/// How a command was launched, and with what: what it receives on every
/// way in. The default is a launch by the user from root search, with
/// nothing more.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchRecord {
    pub launch_type: LaunchType,
    pub source: LaunchSource,
    /// The values of the command's arguments, by name, in the order the
    /// command declares them (see `arguments`). An argument without a
    /// value (an optional one left empty) is absent.
    pub arguments: Vec<(String, String)>,
    /// The text sent through the command's alias or as a fallback,
    /// trimmed and never empty.
    pub fallback_text: Option<String>,
    /// The JSON value, as text, another command passed when it launched
    /// this one.
    pub context: Option<String>,
}

impl LaunchRecord {
    /// A launch by the user from `source`, with nothing more.
    pub fn by_user(source: LaunchSource) -> LaunchRecord {
        LaunchRecord {
            source,
            ..LaunchRecord::default()
        }
    }

    /// A launch by the user from `source` (an alias or a fallback) sending
    /// `text`, trimmed; none when it is blank.
    pub fn sending(source: LaunchSource, text: &str) -> LaunchRecord {
        let text = text.trim();
        LaunchRecord {
            fallback_text: (!text.is_empty()).then(|| text.to_owned()),
            ..LaunchRecord::by_user(source)
        }
    }

    /// A background launch by the command's own schedule.
    pub fn scheduled() -> LaunchRecord {
        LaunchRecord {
            launch_type: LaunchType::Background,
            source: LaunchSource::Schedule,
            ..LaunchRecord::default()
        }
    }

    /// Whether Pane runs the command without a window.
    pub fn is_background(&self) -> bool {
        self.launch_type == LaunchType::Background
    }

    /// The value of the argument `name`, if it has one.
    pub fn argument(&self, name: &str) -> Option<&str> {
        self.arguments
            .iter()
            .find(|(argument, _)| argument == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Another command a command asks Pane to launch (`launch`), and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LaunchRequest {
    /// The component of the calling guest, which names its package.
    pub caller: std::path::PathBuf,
    /// The target's package identity, as an operation call names it; `None`
    /// for the caller's own package.
    pub source: Option<String>,
    /// The target's id in its package's manifest.
    pub command: String,
    pub launch_type: LaunchType,
    pub arguments: Vec<(String, String)>,
    /// JSON text, checked by Pane before the launch.
    pub context: Option<String>,
}

/// What Pane does with a guest's launch request: starts it, or says why it
/// will not. It never waits for the target to run: the target may need the
/// calling instance's turn, which the caller holds until it answers.
pub(crate) type Launches =
    std::sync::Arc<dyn Fn(LaunchRequest) -> Result<(), String> + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sent_text_is_trimmed_and_blank_text_is_none() {
        let sent = LaunchRecord::sending(LaunchSource::Alias, "  hello  world ");
        assert_eq!(sent.fallback_text.as_deref(), Some("hello  world"));
        assert_eq!(sent.launch_type, LaunchType::UserInitiated);
        assert_eq!(sent.source, LaunchSource::Alias);
        let blank = LaunchRecord::sending(LaunchSource::Fallback, "   ");
        assert_eq!(blank.fallback_text, None);
    }

    #[test]
    fn a_scheduled_launch_is_in_the_background() {
        let scheduled = LaunchRecord::scheduled();
        assert!(scheduled.is_background());
        assert_eq!(scheduled.source, LaunchSource::Schedule);
        assert!(!LaunchRecord::default().is_background());
    }
}
