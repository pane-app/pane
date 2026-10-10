//! `pane-ext pack [folder]` (#225, ADR 0047): building a package's release
//! components and assembling what its users will download.
//!
//! The components are built with the same code development mode builds
//! with — the `pane-build` crate's `build_package`, run here once — so the
//! two can never build a package differently, and copied back into the
//! package folder as a development build's are, so the folder itself
//! becomes the release revision a Git-distributed package commits. The
//! folder is then handed to `pane_core::pack`, which gathers the files
//! users download, checks them with Pane's own rules and writes the npm
//! tarball; every message is Pane's, the one install would give. Nothing
//! is published, and nothing reaches the network or the running Pane:
//! publishing is the author's step, never the tool's (ADR 0047).

use std::path::PathBuf;
use std::process::ExitCode;

use pane_build::Builder;
use pane_core::develop::{PaneManifest, Toolchains};
use pane_core::pack::pack_package;

/// Packs the package in `folder`, or the current folder, printing what it
/// built and found and made; the code it returns says whether the package
/// packed.
pub(crate) fn run(folder: Option<PathBuf>) -> ExitCode {
    match pack(folder) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("pane-ext: {message}");
            ExitCode::FAILURE
        }
    }
}

fn pack(folder: Option<PathBuf>) -> Result<(), String> {
    let folder = match folder {
        Some(folder) => folder,
        None => std::env::current_dir()
            .map_err(|error| format!("the current folder cannot be read: {error}"))?,
    };
    let folder = pane_build::canonical(&folder)
        .map_err(|error| format!("{} cannot be packed: {error}", folder.display()))?;
    if !folder.join(pane_build::MANIFEST_FILE).is_file() {
        return Err(format!(
            "{} has no {}, so it is not a package folder",
            folder.display(),
            pane_build::MANIFEST_FILE
        ));
    }
    // The same build development mode runs, once: cargo for a Rust
    // package, Pane's JavaScript build for a JavaScript or TypeScript one.
    let toolchains = Toolchains::from_env(None);
    let command = toolchains
        .build_for(&folder)
        .map_err(|reason| format!("{} cannot be packed: {reason}", folder.display()))?
        .command();
    println!("pane-ext: building {} with `{command}`", folder.display());
    let work = crate::dev::work_folder(&folder);
    let staging = work.join("pack");
    let log = work.join("pack-build.log");
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_file(&log);
    let built = match pane_build::build_package(
        &toolchains,
        &PaneManifest,
        &folder,
        &staging,
        Some(&log),
    ) {
        Ok(built) => built,
        Err(failure) => {
            eprintln!(
                "pane-ext: {} did not build: {}",
                folder.display(),
                failure.summary
            );
            for line in &failure.output {
                eprintln!("{line}");
            }
            if let Some(log) = &failure.log {
                eprintln!("pane-ext: the whole output is in {}", log.display());
            }
            return Err(format!(
                "{} was not packed: it did not build",
                folder.display()
            ));
        }
    };
    // The built components go back into the package folder, as a
    // development build's do: the folder is what its users get, and a
    // later pack rebuilds it in place.
    pane_build::copy_components(&PaneManifest, &staging, &folder);
    for component in &built {
        println!("pane-ext: built {}", component.display());
    }
    let packed = pack_package(&folder);
    for problem in &packed.report.errors {
        println!("error: {}", problem.message);
    }
    for problem in &packed.report.warnings {
        println!("warning: {}", problem.message);
    }
    if !packed.report.ok() {
        let count = packed.report.errors.len();
        let plural = if count == 1 { "" } else { "s" };
        return Err(format!(
            "{} has {count} problem{plural} Pane or npm would refuse it for; fix them and pack \
             again",
            folder.display()
        ));
    }
    if let Some(tarball) = &packed.tarball {
        println!(
            "pane-ext: packed {}: {} files, {} bytes",
            tarball.path.display(),
            tarball.files.len(),
            tarball.bytes
        );
        for file in &tarball.files {
            println!("  {}", file.display());
        }
    } else if let Some(revision) = &packed.revision {
        println!(
            "pane-ext: the release revision {} holds {} files and folders, {} bytes",
            folder.display(),
            revision.entries,
            revision.bytes
        );
    } else {
        return Err(format!("{} was not packed", folder.display()));
    }
    Ok(())
}
