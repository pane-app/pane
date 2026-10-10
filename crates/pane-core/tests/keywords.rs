//! Keywords, alternate titles and same-name rows in root search (#197):
//! a command found by the `keywords` its manifest declares, ranked as its
//! subtitle is, an indexed result found by its alternate titles (as a
//! title is) and its keywords (as a subtitle is), and rows that share a
//! folded title each showing what tells them apart. Through the
//! launcher's public interface with the real sample packages
//! `cargo xtask guests` assembles (`sample-keywords` in Rust, JavaScript
//! and TypeScript), which declare the manifest field and supply the
//! indexed result.

use std::fs;
use std::path::PathBuf;

use futures::executor::block_on;
use pane_core::{Launcher, Runtime, Screen, Status};
use tempfile::TempDir;

#[path = "support/feedback.rs"]
mod feedback;
#[path = "support/rows.rs"]
mod rows;

use feedback::shown;
use rows::titles;

/// Pane's data location and package sources for one test.
struct Dirs {
    data: TempDir,
    sources: TempDir,
}

impl Dirs {
    fn new() -> Dirs {
        Dirs {
            data: tempfile::tempdir().unwrap(),
            sources: tempfile::tempdir().unwrap(),
        }
    }

    fn packages_dir(&self) -> PathBuf {
        self.data.path().join("extensions")
    }

    /// Starts Pane on this data location.
    fn launcher(&self) -> Launcher {
        let runtime = Runtime::start().unwrap();
        Launcher::with_packages(Ok(runtime), vec![], self.packages_dir())
    }

    /// Copies the assembled package `name` under `target/guests/packages`
    /// to the source folder `folder`.
    fn source(&self, name: &str, folder: &str) -> PathBuf {
        let assembled = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/guests/packages")
            .join(name);
        assert!(
            assembled.exists(),
            "{} is missing; run `cargo xtask guests`",
            assembled.display()
        );
        let folder = self.sources.path().join(folder);
        fs::create_dir_all(&folder).unwrap();
        for entry in fs::read_dir(&assembled).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), folder.join(entry.file_name())).unwrap();
        }
        folder
    }

    /// Installs the assembled package `name` from its own copy of it,
    /// `folder`, and returns to root search.
    fn install(&self, launcher: &Launcher, name: &str, folder: &str) -> PathBuf {
        let folder = self.source(name, folder);
        block_on(launcher.install_package(&folder));
        assert!(
            matches!(launcher.view().status, Status::Result(_)),
            "{:?}",
            launcher.view().status
        );
        launcher.back();
        assert!(matches!(launcher.view().screen, Screen::Root { .. }));
        folder
    }
}

fn search(launcher: &Launcher, query: &str) {
    block_on(launcher.set_query(query));
}

/// The subtitles of the rows on screen, in order.
fn subtitles(launcher: &Launcher) -> Vec<String> {
    launcher
        .view()
        .rows
        .into_iter()
        .map(|row| row.subtitle.unwrap_or_default())
        .collect()
}

struct Fixture {
    /// The assembled package under `target/guests/packages`.
    package: &'static str,
    /// The package's title, which stands in as the command's subtitle
    /// when its manifest gives none.
    title: &'static str,
}

const RUST: Fixture = Fixture {
    package: "sample-keywords",
    title: "Keywords sample",
};
const JAVASCRIPT: Fixture = Fixture {
    package: "sample-keywords-js",
    title: "JavaScript keywords sample",
};
const TYPESCRIPT: Fixture = Fixture {
    package: "sample-keywords-ts",
    title: "TypeScript keywords sample",
};

impl Fixture {
    fn pane(&self) -> (Dirs, Launcher) {
        let dirs = Dirs::new();
        let launcher = dirs.launcher();
        dirs.install(&launcher, self.package, "only");
        (dirs, launcher)
    }
}

/// A command is found by the keywords its manifest declares, and its row
/// keeps its own title and subtitle: the words are matched as the
/// subtitle is, and nothing is ever sent for one.
fn a_command_is_found_by_the_keywords_its_manifest_declares(fixture: &Fixture) {
    let (_dirs, launcher) = fixture.pane();

    // "trash" holds no letter of "Empty the Bin", nor of the package
    // title that stands in as its subtitle: only the keyword finds it.
    search(&launcher, "trash");
    assert_eq!(titles(&launcher), ["Empty the Bin"]);
    assert_eq!(
        subtitles(&launcher),
        [fixture.title],
        "the row keeps the subtitle its manifest gives"
    );
    search(&launcher, "rubbish");
    assert_eq!(titles(&launcher), ["Empty the Bin"]);
    // A word of none of them finds nothing, and the title still does.
    search(&launcher, "banana");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    search(&launcher, "bin");
    assert_eq!(titles(&launcher), ["Empty the Bin"]);

    // Invoking the row a keyword found runs the command, as any row: the
    // no-view command says what it did through its toast.
    launcher.select(0);
    block_on(launcher.activate_selected());
    assert_eq!(
        shown(&launcher),
        Status::Result("Emptied the bin".into())
    );
}

/// The indexed result a command supplies is found by its alternate
/// titles as a title is and by its keywords as a subtitle is; the row
/// still shows its title, and the supplying command, a root provider,
/// has no row of its own.
fn an_indexed_result_is_found_by_its_alternate_titles_and_keywords(fixture: &Fixture) {
    let (_dirs, launcher) = fixture.pane();

    // The alternate title, exactly: the query is longer than three
    // characters, so the exact-title step holds it.
    search(&launcher, "luna");
    assert_eq!(titles(&launcher), ["The Moon"]);
    // The keywords, as the subtitle is matched.
    search(&launcher, "satellite");
    assert_eq!(titles(&launcher), ["The Moon"]);
    search(&launcher, "rock");
    assert_eq!(titles(&launcher), ["The Moon"]);
    // The title as ever, exactly and scattered.
    search(&launcher, "the moon");
    assert_eq!(titles(&launcher), ["The Moon"]);
    search(&launcher, "moon");
    assert_eq!(titles(&launcher), ["The Moon"]);
    // A query holding letters of both matches nothing: a keyword is its
    // own text, and no composite spans a title and one.
    search(&launcher, "luna rock");
    assert_eq!(titles(&launcher), Vec::<String>::new());
    // The supplying command is a root provider: no row of its own.
    search(&launcher, "moons");
    assert!(!titles(&launcher).contains(&"Moons".to_owned()));
}

/// Rows that share a folded title each show what tells them apart: two
/// copies of a package, installed from two folders, are two rows of one
/// title, each naming its package's source after its subtitle, and the
/// indexed results of both copies keep the provider's own order, the
/// first copy's ahead of the second's.
#[test]
fn two_copies_of_a_package_each_name_their_source() {
    let dirs = Dirs::new();
    let launcher = dirs.launcher();
    let first = dirs.install(&launcher, "sample-keywords", "first");
    let second = dirs.install(&launcher, "sample-keywords", "second");

    search(&launcher, "trash");
    assert_eq!(
        titles(&launcher),
        ["Empty the Bin", "Empty the Bin"],
        "both copies match by their keyword"
    );
    let shown = subtitles(&launcher);
    assert!(
        shown[0].starts_with("Keywords sample"),
        "the source comes after the subtitle: {shown:?}"
    );
    assert!(
        shown[0].ends_with(&format!("· local folder {}", first.display())),
        "{shown:?}"
    );
    assert!(
        shown[1].ends_with(&format!("· local folder {}", second.display())),
        "{shown:?}"
    );

    search(&launcher, "moon");
    assert_eq!(titles(&launcher), ["The Moon", "The Moon"]);
}

/// Declares one test per check for each language's sample.
macro_rules! contract {
    ($($check:ident),* $(,)?) => {
        mod rust {
            $(#[test] fn $check() { super::$check(&super::RUST) })*
        }
        mod javascript {
            $(#[test] fn $check() { super::$check(&super::JAVASCRIPT) })*
        }
        mod typescript {
            $(#[test] fn $check() { super::$check(&super::TYPESCRIPT) })*
        }
    };
}

contract!(
    a_command_is_found_by_the_keywords_its_manifest_declares,
    an_indexed_result_is_found_by_its_alternate_titles_and_keywords,
);
