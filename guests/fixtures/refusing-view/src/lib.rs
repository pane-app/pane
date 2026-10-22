//! Test fixture: a command that starts, but whose list is always refused
//! with an ordinary error the guest returns, as a command that needs the
//! user to sign in first would. That is an expected operation error, not a
//! failure to start.
#![no_std]

use pane_extension::alloc::{format, string::String, vec::Vec};
use pane_extension::{Command, List};

struct RefusingView;
pane_extension::export!(RefusingView);

impl Command for RefusingView {
    type DesignedView = pane_extension::view::NoDesignedView;

    async fn render() -> Result<List, String> {
        Err("sign in first".into())
    }
}
