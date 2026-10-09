//! Development mode's builds: how a local package's source folder is built
//! on save (ADR 0004; #12, #13). What a build is, and the session that runs
//! one after each save, are the `pane-build` crate's, which `pane-ext`
//! builds with too (ADR 0047, #216); this module gives it Pane's manifest.
//! Its JavaScript and TypeScript build (#218) is `pane-build`'s own, linked
//! in through the dev-dependency below, so Pane's tests build a JS/TS
//! package with Node.js and npm alone.

use std::path::{Path, PathBuf};

pub use pane_build::{Build, BuildJob, BuildOutcome, Builder, Componentizer};

use crate::packages::Manifest;

/// Pane's builds, reading a package's manifest as Pane does.
pub type Toolchains = pane_build::Toolchains<PaneManifest>;

/// What a package's `pane.json` names, read by Pane's own rules.
#[derive(Clone, Copy, Debug, Default)]
pub struct PaneManifest;

impl pane_build::ManifestFiles for PaneManifest {
    fn components(&self, folder: &Path) -> Result<Vec<PathBuf>, String> {
        let (manifest, _) = Manifest::read_parsed(folder).map_err(|error| error.to_string())?;
        let mut components: Vec<PathBuf> = Vec::new();
        for (_, component) in manifest.components() {
            if !components.iter().any(|known| known == component) {
                components.push(component.to_path_buf());
            }
        }
        Ok(components)
    }

    fn helper_files(&self, folder: &Path) -> Result<Vec<PathBuf>, String> {
        let (manifest, _) = Manifest::read_parsed(folder).map_err(|error| error.to_string())?;
        Ok(manifest
            .helpers
            .iter()
            .filter_map(|helper| helper.for_this_system())
            .map(Path::to_path_buf)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use pane_build::{is_save, stage_package};

    use super::*;

    fn manifest(folder: &Path, component: &str) {
        std::fs::write(
            folder.join("pane.json"),
            format!(
                r#"{{ "manifestVersion": 1, "title": "Hello", "apiVersion": "0.1",
                     "commands": [{{ "id": "hello", "title": "Hello", "component": "{component}" }}] }}"#
            ),
        )
        .unwrap();
    }

    fn toolchains() -> Toolchains {
        Toolchains {
            cargo: Some("cargo".into()),
            // In this process, as Pane's tests build with (pane-build's
            // `componentizer` feature, a dev-dependency of pane-core's).
            componentizer: Componentizer::Linked,
            manifests: PaneManifest,
        }
    }

    #[test]
    fn a_folder_with_cargo_toml_builds_with_cargo_and_ignores_its_output() {
        let folder = tempfile::tempdir().unwrap();
        manifest(folder.path(), "target/wasm32-wasip2/release/hello.wasm");
        std::fs::write(folder.path().join("Cargo.toml"), "").unwrap();
        let build = toolchains().build_for(folder.path()).unwrap();
        assert_eq!(
            build.command(),
            "cargo build --release --target wasm32-wasip2 --message-format=json-render-diagnostics"
        );
        for output in [
            "target",
            "target/wasm32-wasip2/release/hello.wasm",
            "Cargo.lock",
            "crates/inner/target/debug/x",
            "web/node_modules/zod/index.js",
            "web/dist/app.js",
        ] {
            assert!(!is_save(Path::new(output), &*build), "{output}");
        }
        for source in ["src/lib.rs", "Cargo.toml", "pane.json", "wit/world.wit"] {
            assert!(is_save(Path::new(source), &*build), "{source}");
        }
    }

    #[test]
    fn a_folder_with_package_json_builds_with_pane_js() {
        let folder = tempfile::tempdir().unwrap();
        manifest(folder.path(), "dist/hello.wasm");
        std::fs::write(folder.path().join("package.json"), "{}").unwrap();
        std::fs::write(folder.path().join("tsconfig.json"), "{}").unwrap();
        let build = toolchains().build_for(folder.path()).unwrap();
        assert_eq!(
            build.command(),
            "npm ci --ignore-scripts && tsc -p tsconfig.json && esbuild --bundle && componentize"
        );
        for output in ["dist", "dist/hello.wasm", "node_modules/zod/index.js"] {
            assert!(!is_save(Path::new(output), &*build), "{output}");
        }
        for source in ["src/index.ts", "package.json", "tsconfig.json", "pane.json"] {
            assert!(is_save(Path::new(source), &*build), "{source}");
        }
        // Without a tsconfig there is no type-checking step.
        std::fs::remove_file(folder.path().join("tsconfig.json")).unwrap();
        let build = toolchains().build_for(folder.path()).unwrap();
        assert_eq!(
            build.command(),
            "npm ci --ignore-scripts && esbuild --bundle && componentize"
        );
    }

    #[test]
    fn hidden_files_and_editor_temporaries_are_not_saves() {
        let folder = tempfile::tempdir().unwrap();
        manifest(folder.path(), "hello.wasm");
        std::fs::write(folder.path().join("Cargo.toml"), "").unwrap();
        let build = toolchains().build_for(folder.path()).unwrap();
        for path in [
            ".git/index",
            "src/.lib.rs.swp",
            "src/lib.rs.swp",
            "src/4913",
            "src/lib.rs~",
            "src/lib.rs___jb_tmp___",
            "src/.#lib.rs",
            "src/#lib.rs#",
            "hello.wasm",
            "",
        ] {
            assert!(!is_save(Path::new(path), &*build), "{path}");
        }
    }

    #[test]
    fn a_folder_without_a_known_build_or_tool_is_explained() {
        let folder = tempfile::tempdir().unwrap();
        manifest(folder.path(), "hello.wasm");
        let error = toolchains().build_for(folder.path()).err().unwrap();
        assert!(error.contains("neither Cargo.toml"), "{error}");
        std::fs::write(folder.path().join("package.json"), "{}").unwrap();
        // A package without Pane's CLI installed, when no componentizer of
        // Pane's own is linked in or named: run npm install.
        let no_cli = Toolchains {
            componentizer: Componentizer::Binary(None),
            ..toolchains()
        };
        let error = no_cli.build_for(folder.path()).err().unwrap();
        assert!(error.contains("npm install"), "{error}");
        assert!(error.contains("node_modules/@pane-app/cli-"), "{error}");
        // A componentizer folder that holds none is explained too.
        let empty = Toolchains {
            componentizer: Componentizer::Binary(Some(PathBuf::from("/pane/componentizer"))),
            ..no_cli
        };
        let error = empty.build_for(folder.path()).err().unwrap();
        assert!(error.contains("PANE_COMPONENTIZER"), "{error}");
        std::fs::write(folder.path().join("Cargo.toml"), "").unwrap();
        let no_cargo = Toolchains {
            cargo: None,
            ..toolchains()
        };
        let error = no_cargo.build_for(folder.path()).err().unwrap();
        assert!(
            error.contains("cargo on PATH, then ~/.cargo/bin/cargo"),
            "{error}"
        );
    }

    #[test]
    fn staging_takes_pane_json_and_this_systems_helper_files() {
        let target = pane_target::Target::current().expect("Pane names this system's target");
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("source");
        let file = format!("helpers/tool{}", target.exe_suffix());
        let other = if target.id() == "linux-x86_64" {
            "linux-aarch64"
        } else {
            "linux-x86_64"
        };
        std::fs::create_dir_all(folder.join("helpers")).unwrap();
        std::fs::write(folder.join(&file), b"a program").unwrap();
        std::fs::write(folder.join("helpers/other"), b"another system's").unwrap();
        std::fs::write(
            folder.join("pane.json"),
            format!(
                r#"{{ "manifestVersion": 1, "title": "Tool", "apiVersion": "0.1",
                     "commands": [{{ "id": "c", "title": "C", "component": "command.wasm" }}],
                     "helpers": [{{ "id": "tool", "targets": {{ "{}": "{file}", "{other}": "helpers/other" }} }}] }}"#,
                target.id()
            ),
        )
        .unwrap();
        let staging = dir.path().join("staging");
        stage_package(&PaneManifest, &folder, &staging).unwrap();
        assert!(staging.join("pane.json").is_file());
        assert_eq!(std::fs::read(staging.join(&file)).unwrap(), b"a program");
        assert!(!staging.join("helpers/other").exists());
        // The build adds the components.
        assert!(!staging.join("command.wasm").exists());
    }
}
