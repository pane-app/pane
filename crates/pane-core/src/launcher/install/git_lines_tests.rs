//! The preview's lines about a Git revision, for addresses the tests'
//! 127.0.0.1 servers cannot stand for (SSH ones, fetched over HTTPS).

use super::git_lines;
use crate::git::{GitOrigin, GitRef, GitRevision, GitSpec};

fn origin(address: &str, reference: GitRef, advertised: bool) -> GitOrigin {
    GitOrigin {
        repository: GitSpec::parse(address).unwrap().repository,
        revision: GitRevision {
            reference,
            commit: "1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b".into(),
        },
        advertised,
        subject: "Release 1.0.0".into(),
        lfs_pointers: Vec::new(),
    }
}

#[test]
fn an_ssh_address_is_said_to_be_fetched_over_https() {
    let lines = git_lines(
        &origin(
            "git@github.com:Owner/Repo.git",
            GitRef::Tag("v1".into()),
            true,
        ),
        None,
    );
    assert!(
        lines.contains(
            &"Fetched: commit 1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b “Release 1.0.0”, SSH \
              address fetched over HTTPS from https://github.com/Owner/Repo.git; each object \
              checked against its id"
                .to_owned()
        ),
        "{lines:#?}"
    );
    let lines = git_lines(
        &origin(
            "https://github.com/Owner/Repo.git",
            GitRef::Tag("v1".into()),
            true,
        ),
        None,
    );
    assert!(
        lines.contains(
            &"Fetched: commit 1a2b3c4d5e6f1a2b3c4d5e6f1a2b3c4d5e6f1a2b “Release 1.0.0”, served \
              at https://github.com/Owner/Repo.git; each object checked against its id"
                .to_owned()
        ),
        "{lines:#?}"
    );
}
