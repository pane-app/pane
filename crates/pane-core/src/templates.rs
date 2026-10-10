//! The templates `pane-ext new` writes (#221, ADR 0047): the four the ADR
//! settles — `list`, `detail`, `form` and `no-view` — in Rust and in
//! TypeScript, embedded from the committed files under `templates/` with
//! `include_str!`, so what `pane-ext new` (and the app's Create Extension
//! command, when it lands) writes is what this repository holds. The
//! placeholder `icon.png` is not a file there: it is written as a
//! deterministic 512×512 PNG, the size a published extension's icon is.
//!
//! A template's folder holds exactly what a fresh package needs: `pane.json`
//! (with a `"$schema"` Pane ignores, as it ignores every field it does not
//! know, but an editor checks), the command's source, a README, the icon,
//! `.gitignore`, and either a `package.json` (naming `@pane-app/extension`
//! and `@pane-app/cli` from npm, with `dev`, `check` and `pack` scripts
//! that run `pane-ext`) or a `Cargo.toml` (naming the `pane-extension`
//! crate from crates.io, with a stable toolchain file), plus the
//! formatter, linter and type-checker configuration the language uses.
//!
//! One component serves every command a package declares (`render` and
//! `run` receive the command's id, `wit/extension.wit`), so each template's
//! entry file matches on the command id, with a comment marking where
//! [`add_command`] adds an arm and where it adds the module it calls:
//! adding a command is an honest, small edit of the author's own file, and
//! the package still builds and its commands still run afterwards.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::check::title_case;
use crate::icons;
use crate::packages::{MANIFEST_FILE, Manifest};

/// A template's kind, as `--template` names it: what the command it writes
/// does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A command that opens a list of items, each running an action.
    List,
    /// A command that takes a query and shows the detail of the text root
    /// search sends it.
    Detail,
    /// A command whose item opens a form, which it answers.
    Form,
    /// A command that runs without a screen and answers in a toast.
    NoView,
}

impl Kind {
    /// The kind `text` names, as `--template` spells it.
    pub fn parse(text: &str) -> Option<Kind> {
        match text.trim().to_ascii_lowercase().as_str() {
            "list" => Some(Kind::List),
            "detail" => Some(Kind::Detail),
            "form" => Some(Kind::Form),
            "no-view" => Some(Kind::NoView),
            _ => None,
        }
    }

    /// The kind's name, as `--template` spells it and the templates'
    /// folders are named.
    pub fn name(self) -> &'static str {
        match self {
            Kind::List => "list",
            Kind::Detail => "detail",
            Kind::Form => "form",
            Kind::NoView => "no-view",
        }
    }

    /// Every kind, as `--template`'s help lists them.
    pub const ALL: [Kind; 4] = [Kind::List, Kind::Detail, Kind::Form, Kind::NoView];
}

/// The language a template is written in, as `--language` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    /// Rust, with the `pane-extension` SDK.
    Rust,
    /// TypeScript, with the `@pane-app/extension` SDK.
    TypeScript,
}

impl Language {
    /// The language `text` names, as `--language` spells it.
    pub fn parse(text: &str) -> Option<Language> {
        match text.trim().to_ascii_lowercase().as_str() {
            "rust" => Some(Language::Rust),
            "typescript" | "ts" => Some(Language::TypeScript),
            _ => None,
        }
    }

    /// The language's name, as its README and `--language`'s help spell it.
    pub fn name(self) -> &'static str {
        match self {
            Language::Rust => "Rust",
            Language::TypeScript => "TypeScript",
        }
    }
}

/// The language of the package in `folder`, as its entry file says: a Rust
/// package (`Cargo.toml` with `src/lib.rs`) or a TypeScript one
/// (`package.json` with `src/index.ts`). `None` for anything else, which
/// [`add_command`] refuses by name.
pub fn language_of(folder: &Path) -> Option<Language> {
    if folder.join("Cargo.toml").is_file() && folder.join("src/lib.rs").is_file() {
        Some(Language::Rust)
    } else if folder.join("package.json").is_file() && folder.join("src/index.ts").is_file() {
        Some(Language::TypeScript)
    } else {
        None
    }
}

/// The name an author gives an extension, in the spellings a package uses:
/// its title in Title Case, and its package name (and first command's id)
/// in kebab case. The Rust crate the package builds and the type the SDK
/// exports are derived from the package name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    /// The package's title, in Title Case as Pane's own are.
    pub title: String,
    /// The package name (npm's, and the command's id), in kebab case.
    pub package: String,
}

impl Name {
    /// The name the author gave, as `--name` (or a folder) spells it.
    /// Letters and digits become the package name, other characters
    /// separate its words; the title keeps what was written in Title Case,
    /// minus the two characters that would break the string literals the
    /// sources hold it in.
    pub fn parse(given: &str) -> Result<Name, String> {
        let title: String = given.chars().filter(|c| *c != '"' && *c != '\\').collect();
        let title = title_case(title.trim());
        let Some(package) = kebab(given) else {
            return Err(format!(
                "\"{given}\" has no letters or digits in it, so it names no package; give the \
                 extension a name such as \"Word Count\""
            ));
        };
        if !package.starts_with(|c: char| c.is_ascii_alphabetic()) {
            return Err(format!(
                "\"{given}\" becomes the package name `{package}`, which does not start with a \
                 letter; give the extension a name starting with a letter, so the Rust \
                 templates' crate names are valid too"
            ));
        }
        Ok(Name { title, package })
    }

    /// The Rust crate's name: the package name as a crate spells it, and
    /// the artifact `cargo build` writes (`__CRATE__.wasm`).
    fn crate_name(&self) -> String {
        self.package.replace('-', "_")
    }

    /// The Rust type the SDK exports the command of (`__STRUCT__`).
    fn type_name(&self) -> String {
        let mut name = String::new();
        for word in self.package.split('-') {
            let mut letters = word.chars();
            match letters.next() {
                Some(first) => {
                    name.extend(first.to_uppercase());
                    name.push_str(letters.as_str());
                }
                None => {}
            }
        }
        name
    }
}

/// `given` as a package name in kebab case: letters and digits
/// lowercased, every run of other characters one `-`, none leading or
/// trailing, at most npm's 214 characters. `None` when nothing is left.
fn kebab(given: &str) -> Option<String> {
    let mut name = String::new();
    for character in given.chars() {
        if character.is_ascii_alphanumeric() {
            name.push(character.to_ascii_lowercase());
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    let name = name.trim_matches('-').to_owned();
    (!name.is_empty() && name.len() <= 214).then_some(name)
}

/// A command [`add_command`] adds, by the id `pane.json` names it by and
/// the title root search shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewCommand {
    /// The command's id, unique in the package.
    pub id: String,
    /// The command's title, in Title Case.
    pub title: String,
}

impl NewCommand {
    /// The command `id`/`title` describe, as `pane-ext new command`'s
    /// flags give them: the id is the title's when only the title is
    /// given, and the title the id's Title Case when only the id is.
    pub fn parse(id: Option<&str>, title: Option<&str>) -> Result<NewCommand, String> {
        let id = match id {
            Some(id) => kebab(id).ok_or_else(|| {
                format!(
                    "\"{id}\" has no letters or digits in it, so it names no command; give the \
                     command an id such as \"word-count\""
                )
            }),
            None => title.and_then(kebab).ok_or_else(|| match title {
                Some(title) => format!(
                    "\"{title}\" has no letters or digits in it, so it names no command; give \
                     the command an id or a title such as \"Word Count\""
                ),
                None => "a command needs --id or --title to name it by".into(),
            }),
        }?;
        let title = match title {
            Some(title) => title_case(title),
            None => title_case(&id.replace('-', " ")),
        };
        Ok(NewCommand { id, title })
    }
}

/// Writes a fresh package from the `kind` template in `language` into
/// `folder`, named `name`, with the placeholder icon. An existing folder is
/// written into only while empty: an author's files are never overwritten.
pub fn scaffold(folder: &Path, name: &Name, language: Language, kind: Kind) -> Result<(), String> {
    if folder.is_file() {
        return Err(format!(
            "{} is a file, not a folder, so a new package is not written into it",
            folder.display()
        ));
    }
    if folder.is_dir() {
        let empty = fs::read_dir(folder)
            .map_err(|error| format!("{} cannot be read: {error}", folder.display()))?
            .next()
            .is_none();
        if !empty {
            return Err(format!(
                "{} is not empty, so a new package is not written into it; move its files \
                 aside or choose another folder",
                folder.display()
            ));
        }
    } else if let Err(error) = fs::create_dir_all(folder) {
        return Err(format!("{} cannot be created: {error}", folder.display()));
    }
    let template = template(language, kind);
    let files = template
        .files()
        .into_iter()
        .chain(shared(language).iter().copied());
    for (path, contents) in files {
        write(folder, path, contents, name)?;
    }
    let icon = folder.join("icon.png");
    fs::write(&icon, placeholder_icon())
        .map_err(|error| format!("{} cannot be written: {error}", icon.display()))?;
    Ok(())
}

/// Adds the command to the package in `folder`: an entry in its
/// `pane.json` (served by the component its other commands are), a source
/// file beside the entry file's, and the entry file's dispatch arms
/// calling it — at the markers the templates write, so the package still
/// builds and the command runs without the author touching anything.
pub fn add_command(folder: &Path, command: &NewCommand, kind: Kind) -> Result<(), String> {
    let manifest = match Manifest::read_parsed(folder) {
        Ok((manifest, _)) => manifest,
        Err(error) => {
            return Err(format!(
                "{} is not a package Pane would read: {error}",
                folder.display()
            ));
        }
    };
    let Some(language) = language_of(folder) else {
        return Err(format!(
            "{} has neither the Rust nor the TypeScript entry file the templates write \
             (src/lib.rs or src/index.ts), so pane-ext cannot add a command to it; add one by \
             hand instead",
            folder.display()
        ));
    };
    let Some(component) = manifest.commands.first().map(|command| &command.component) else {
        return Err(format!(
            "{} declares no command, so there is no component to serve a new one; name one in \
             {} first",
            folder.display(),
            MANIFEST_FILE
        ));
    };
    if manifest.commands.iter().any(|known| known.id == command.id) {
        return Err(format!(
            "{} already has a command named `{}`, so another is not added; give the command \
             another id",
            folder.display(),
            command.id
        ));
    }
    let entry = folder.join(entry_file(language));
    let source = match fs::read_to_string(&entry) {
        Ok(source) => source,
        Err(error) => {
            return Err(format!("{} cannot be read: {error}", entry.display()));
        }
    };
    let identifier = identifier(language, &command.id);
    let module = match language {
        Language::Rust => format!("mod {};", module_name(&command.id)),
        Language::TypeScript => format!("import * as {identifier} from \"./{}\";", command.id),
    };
    let mut source = insert_after_marker(&source, module_marker(), &module)?;
    // The arms the kind's dispatches get: a view command's in `render`, a
    // form's in `submit_form` too, a no-view's in `run`.
    if kind != Kind::NoView {
        source = insert_after_marker(
            &source,
            view_marker(),
            &render_arm(language, &command.id, &identifier),
        )?;
    }
    if kind == Kind::Form {
        source = insert_after_marker(
            &source,
            form_marker(),
            &form_arm(language, &command.id, &identifier),
        )?;
    }
    if kind == Kind::NoView {
        source = insert_after_marker(
            &source,
            no_view_marker(),
            &run_arm(language, &command.id, &identifier),
        )?;
    }
    if let Err(error) = fs::write(&entry, source) {
        return Err(format!("{} cannot be written: {error}", entry.display()));
    }
    // The command's own file, after the entry file's dispatch calls it, so
    // a refusal above leaves nothing behind.
    let (path, contents) = command_file(language, kind);
    let command_name = Name {
        title: command.title.clone(),
        package: command.id.clone(),
    };
    write(folder, &path, contents, &command_name)?;
    add_to_manifest(folder, language, kind, command, component)
}

/// The `pane.json` entry [`add_command`] appends, built from the kind's
/// own template so its subtitle, mode and `takesQuery` come from one
/// place: the template's command, with the new command's id and title and
/// the package's own component.
fn add_to_manifest(
    folder: &Path,
    language: Language,
    kind: Kind,
    command: &NewCommand,
    component: &Path,
) -> Result<(), String> {
    let file = folder.join(MANIFEST_FILE);
    let manifest = fs::read_to_string(&file)
        .map_err(|error| format!("{} cannot be read: {error}", file.display()))?;
    let mut manifest: Value = serde_json::from_str(&manifest)
        .map_err(|error| format!("{} is not valid JSON: {error}", file.display()))?;
    let Some(commands) = manifest.get_mut("commands").and_then(Value::as_array_mut) else {
        return Err(format!(
            "{} declares no commands array, so a command is not added to it",
            file.display()
        ));
    };
    let template = serde_json::from_str::<Value>(template(language, kind).pane)
        .expect("the embedded pane.json is valid JSON");
    let mut entry = template["commands"][0]
        .as_object()
        .expect("the template's command is an object")
        .clone();
    entry.insert("id".into(), json!(command.id));
    entry.insert("title".into(), json!(command.title));
    entry.insert(
        "component".into(),
        json!(component.to_string_lossy().replace('\\', "/")),
    );
    commands.push(Value::Object(entry));
    let mut written = serde_json::to_string_pretty(&manifest)
        .expect("a manifest of JSON values is always writable");
    written.push('\n');
    fs::write(&file, written)
        .map_err(|error| format!("{} cannot be written: {error}", file.display()))
}

/// The entry file of a package in `language`.
fn entry_file(language: Language) -> &'static str {
    match language {
        Language::Rust => "src/lib.rs",
        Language::TypeScript => "src/index.ts",
    }
}

/// The module name of the command `id`: a Rust module's name.
fn module_name(id: &str) -> String {
    id.replace('-', "_")
}

/// The identifier the entry file refers to the command by: a Rust module's
/// name, or (because a hyphen cannot be in a TypeScript identifier) the
/// id's words in camel case.
fn identifier(language: Language, id: &str) -> String {
    match language {
        Language::Rust => module_name(id),
        Language::TypeScript => {
            let mut name = String::new();
            for (at, word) in id.split('-').enumerate() {
                let mut letters = word.chars();
                match letters.next() {
                    Some(first) => {
                        if at > 0 {
                            name.extend(first.to_uppercase());
                        } else {
                            name.push(first);
                        }
                        name.push_str(letters.as_str());
                    }
                    None => {}
                }
            }
            name
        }
    }
}

/// The arm the entry file's `render` dispatch gets for a view command.
fn render_arm(language: Language, id: &str, identifier: &str) -> String {
    match language {
        Language::Rust => format!("\"{id}\" => {identifier}::render().await,"),
        Language::TypeScript => format!("case \"{id}\": return {identifier}.render(launch);"),
    }
}

/// The arm the entry file's `run` dispatch gets for a no-view command.
fn run_arm(language: Language, id: &str, identifier: &str) -> String {
    match language {
        Language::Rust => format!("\"{id}\" => {identifier}::run(_launch).await,"),
        Language::TypeScript => format!("case \"{id}\": return {identifier}.run(_launch);"),
    }
}

/// The arm the entry file's `submit_form`/`submitForm` dispatch gets for a
/// form command.
fn form_arm(language: Language, id: &str, identifier: &str) -> String {
    match language {
        Language::Rust => format!("\"{id}\" => {identifier}::submit_form(item_id, _values).await,"),
        Language::TypeScript => {
            format!("case \"{id}\": return {identifier}.submit(itemId, _values);")
        }
    }
}

/// Where `add_command` adds the command's module or import.
fn module_marker() -> &'static str {
    "pane-ext new command adds a command's module here."
}

/// Where it adds a view command's arm.
fn view_marker() -> &'static str {
    "pane-ext new command adds a view command's arm here."
}

/// Where it adds a no-view command's arm.
fn no_view_marker() -> &'static str {
    "pane-ext new command adds a no-view command's arm here."
}

/// Where it adds a form command's arm.
fn form_marker() -> &'static str {
    "pane-ext new command adds a form command's arm here."
}

/// `source` with `added` on its own line after the line holding `marker`,
/// with the marker's own indentation; or why the marker is gone, so a
/// command is not added silently halfway.
fn insert_after_marker(source: &str, marker: &str, added: &str) -> Result<String, String> {
    let Some(at) = source.find(marker) else {
        return Err(format!(
            "the entry file no longer carries pane-ext's marker `{marker}`, so the command \
             cannot be added to it; add it by hand instead"
        ));
    };
    let line_end = source[at..].find('\n').map_or(source.len(), |end| at + end + 1);
    let indent = source[..at].rfind('\n').map_or(0, |start| start + 1);
    let indent = &source[indent..at];
    let mut written = String::with_capacity(source.len() + added.len() + 1);
    written.push_str(&source[..line_end]);
    written.push_str(indent);
    written.push_str(added);
    written.push('\n');
    written.push_str(&source[line_end..]);
    Ok(written)
}

/// One package template's own files: its `pane.json`, its package manifest
/// (`Cargo.toml` or `package.json`), its README and its source, each by
/// the path it is written to.
struct Template {
    pane: &'static str,
    package_file: &'static str,
    package: &'static str,
    readme: &'static str,
    source_file: &'static str,
    contents: &'static str,
}

impl Template {
    /// The template's own files, by the path they are written to; the
    /// shared files of its language come after them.
    fn files(&self) -> [(&'static str, &'static str); 4] {
        [
            ("pane.json", self.pane),
            (self.package_file, self.package),
            ("README.md", self.readme),
            (self.source_file, self.contents),
        ]
    }
}

/// The template `language` and `kind` name.
fn template(language: Language, kind: Kind) -> Template {
    match (language, kind) {
        (Language::Rust, Kind::List) => Template {
            pane: include_str!("../templates/rust/list/pane.json"),
            package_file: "Cargo.toml",
            package: include_str!("../templates/rust/list/Cargo.toml"),
            readme: include_str!("../templates/rust/list/README.md"),
            source_file: "src/lib.rs",
            contents: include_str!("../templates/rust/list/src/lib.rs"),
        },
        (Language::Rust, Kind::Detail) => Template {
            pane: include_str!("../templates/rust/detail/pane.json"),
            package_file: "Cargo.toml",
            package: include_str!("../templates/rust/detail/Cargo.toml"),
            readme: include_str!("../templates/rust/detail/README.md"),
            source_file: "src/lib.rs",
            contents: include_str!("../templates/rust/detail/src/lib.rs"),
        },
        (Language::Rust, Kind::Form) => Template {
            pane: include_str!("../templates/rust/form/pane.json"),
            package_file: "Cargo.toml",
            package: include_str!("../templates/rust/form/Cargo.toml"),
            readme: include_str!("../templates/rust/form/README.md"),
            source_file: "src/lib.rs",
            contents: include_str!("../templates/rust/form/src/lib.rs"),
        },
        (Language::Rust, Kind::NoView) => Template {
            pane: include_str!("../templates/rust/no-view/pane.json"),
            package_file: "Cargo.toml",
            package: include_str!("../templates/rust/no-view/Cargo.toml"),
            readme: include_str!("../templates/rust/no-view/README.md"),
            source_file: "src/lib.rs",
            contents: include_str!("../templates/rust/no-view/src/lib.rs"),
        },
        (Language::TypeScript, Kind::List) => Template {
            pane: include_str!("../templates/typescript/list/pane.json"),
            package_file: "package.json",
            package: include_str!("../templates/typescript/list/package.json"),
            readme: include_str!("../templates/typescript/list/README.md"),
            source_file: "src/index.ts",
            contents: include_str!("../templates/typescript/list/src/index.ts"),
        },
        (Language::TypeScript, Kind::Detail) => Template {
            pane: include_str!("../templates/typescript/detail/pane.json"),
            package_file: "package.json",
            package: include_str!("../templates/typescript/detail/package.json"),
            readme: include_str!("../templates/typescript/detail/README.md"),
            source_file: "src/index.ts",
            contents: include_str!("../templates/typescript/detail/src/index.ts"),
        },
        (Language::TypeScript, Kind::Form) => Template {
            pane: include_str!("../templates/typescript/form/pane.json"),
            package_file: "package.json",
            package: include_str!("../templates/typescript/form/package.json"),
            readme: include_str!("../templates/typescript/form/README.md"),
            source_file: "src/index.ts",
            contents: include_str!("../templates/typescript/form/src/index.ts"),
        },
        (Language::TypeScript, Kind::NoView) => Template {
            pane: include_str!("../templates/typescript/no-view/pane.json"),
            package_file: "package.json",
            package: include_str!("../templates/typescript/no-view/package.json"),
            readme: include_str!("../templates/typescript/no-view/README.md"),
            source_file: "src/index.ts",
            contents: include_str!("../templates/typescript/no-view/src/index.ts"),
        },
    }
}

/// The command file [`add_command`] writes for `kind` in `language`, by its
/// path and its contents.
fn command_file(language: Language, kind: Kind) -> (String, &'static str) {
    match (language, kind) {
        (Language::Rust, Kind::List) => (
            "src/list.rs".into(),
            include_str!("../templates/rust/command/list.rs"),
        ),
        (Language::Rust, Kind::Detail) => (
            "src/detail.rs".into(),
            include_str!("../templates/rust/command/detail.rs"),
        ),
        (Language::Rust, Kind::Form) => (
            "src/form.rs".into(),
            include_str!("../templates/rust/command/form.rs"),
        ),
        (Language::Rust, Kind::NoView) => (
            "src/no_view.rs".into(),
            include_str!("../templates/rust/command/no-view.rs"),
        ),
        (Language::TypeScript, Kind::List) => (
            "src/list.ts".into(),
            include_str!("../templates/typescript/command/list.ts"),
        ),
        (Language::TypeScript, Kind::Detail) => (
            "src/detail.ts".into(),
            include_str!("../templates/typescript/command/detail.ts"),
        ),
        (Language::TypeScript, Kind::Form) => (
            "src/form.ts".into(),
            include_str!("../templates/typescript/command/form.ts"),
        ),
        (Language::TypeScript, Kind::NoView) => (
            "src/no-view.ts".into(),
            include_str!("../templates/typescript/command/no-view.ts"),
        ),
    }
}

/// The files every template of `language` writes unchanged: the
/// toolchain, formatter, linter and type-checker configuration, and
/// `.gitignore`.
fn shared(language: Language) -> &'static [(&'static str, &'static str)] {
    match language {
        Language::Rust => &[
            (".gitignore", include_str!("../templates/rust/.gitignore")),
            (
                "rust-toolchain.toml",
                include_str!("../templates/rust/rust-toolchain.toml"),
            ),
            ("rustfmt.toml", include_str!("../templates/rust/rustfmt.toml")),
        ],
        Language::TypeScript => &[
            (
                ".gitignore",
                include_str!("../templates/typescript/.gitignore"),
            ),
            (
                ".prettierrc.json",
                include_str!("../templates/typescript/.prettierrc.json"),
            ),
            (
                "eslint.config.js",
                include_str!("../templates/typescript/eslint.config.js"),
            ),
            (
                "tsconfig.json",
                include_str!("../templates/typescript/tsconfig.json"),
            ),
        ],
    }
}

/// The placeholder icon every scaffold carries: a 512×512 PNG, the size a
/// published extension's icon is, deterministic (a plain two-tone square),
/// so every scaffold of every template holds the same bytes without a
/// binary asset in the repository. The README tells the author to replace
/// it.
fn placeholder_icon() -> Vec<u8> {
    let side = icons::PUBLISHED_ICON_SIZE;
    let pixels = side as usize;
    let mut rgba = vec![0u8; pixels * pixels * 4];
    // A plain two-tone square: a lighter inner square on its frame. The
    // shade is arithmetic, so every scaffold of every template holds the
    // same bytes.
    for (at, pixel) in rgba.chunks_exact_mut(4).enumerate() {
        let square = 64..pixels - 64;
        let inner = square.contains(&(at % pixels)) && square.contains(&(at / pixels));
        let shade = u8::from(inner) * 60;
        pixel.copy_from_slice(&[44 + shade, 84 + shade, 122 + shade, 255]);
    }
    icons::encode_png(side, side, &rgba).expect("a 512×512 image always encodes")
}

/// `contents` with the placeholders replaced by `name`'s spellings.
fn written(contents: &str, name: &Name) -> String {
    contents
        .replace("__TITLE__", &name.title)
        .replace("__NAME__", &name.package)
        .replace("__CRATE__", &name.crate_name())
        .replace("__STRUCT__", &name.type_name())
}

/// Writes `contents`, substituted for `name`, to `folder`/`path`.
fn write(folder: &Path, path: &str, contents: &str, name: &Name) -> Result<(), String> {
    let file = folder.join(path);
    if let Some(parent) = file.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return Err(format!("{} cannot be created: {error}", parent.display()));
        }
    }
    fs::write(&file, written(contents, name))
        .map_err(|error| format!("{} cannot be written: {error}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check;
    use crate::packages::CommandMode;

    /// Every file under `folder`, relative paths in order, without the
    /// folder itself.
    fn files_under(folder: &Path) -> Vec<String> {
        fn walk(folder: &Path, prefix: String, files: &mut Vec<String>) {
            for entry in fs::read_dir(folder).unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = format!("{prefix}{name}");
                if entry.file_type().unwrap().is_dir() {
                    walk(&entry.path(), format!("{path}/"), files);
                } else {
                    files.push(path);
                }
            }
        }
        let mut files = Vec::new();
        walk(folder, String::new(), &mut files);
        files.sort();
        files
    }

    /// The files a scaffold of `language` writes.
    fn expected(language: Language) -> Vec<String> {
        let files: &[&str] = match language {
            Language::Rust => &[
                ".gitignore",
                "Cargo.toml",
                "README.md",
                "icon.png",
                "pane.json",
                "rust-toolchain.toml",
                "rustfmt.toml",
                "src/lib.rs",
            ],
            Language::TypeScript => &[
                ".gitignore",
                ".prettierrc.json",
                "README.md",
                "eslint.config.js",
                "icon.png",
                "package.json",
                "pane.json",
                "src/index.ts",
                "tsconfig.json",
            ],
        };
        files.iter().map(|file| (*file).to_owned()).collect()
    }

    /// A scaffold of every template in both languages: exactly the files a
    /// package needs, a manifest Pane reads with the name in it, a
    /// 512×512 icon, no placeholder left, and nothing Pane's own checks
    /// would refuse the package for but its unbuilt component.
    #[test]
    fn every_template_scaffolds_a_package_pane_would_install_once_built() {
        for language in [Language::Rust, Language::TypeScript] {
            for kind in Kind::ALL {
                let folder = tempfile::tempdir().unwrap();
                let name = Name::parse("Word Count").unwrap();
                scaffold(folder.path(), &name, language, kind).unwrap();
                let files = files_under(folder.path());
                assert_eq!(files, expected(language), "{language:?} {kind:?}");
                for file in &files {
                    let path = folder.path().join(file);
                    let contents = fs::read_to_string(&path).unwrap_or_default();
                    assert!(!contents.contains("__"), "{file} keeps a placeholder: {contents}");
                }
                let (manifest, _) = Manifest::read_parsed(folder.path()).unwrap();
                assert_eq!(manifest.title, "Word Count");
                assert_eq!(manifest.commands.len(), 1, "{kind:?}");
                let command = &manifest.commands[0];
                assert_eq!(command.id, "word-count");
                assert_eq!(command.title, "Word Count");
                assert!(manifest.description.is_some());
                assert!(manifest.icon.is_some());
                let component = match language {
                    Language::Rust => "target/wasm32-wasip2/release/word_count.wasm",
                    Language::TypeScript => "dist/word-count.wasm",
                };
                assert_eq!(command.component, Path::new(component));
                assert_eq!(
                    command.takes_query,
                    matches!(kind, Kind::Detail | Kind::NoView),
                    "{kind:?}"
                );
                let mode = if kind == Kind::NoView {
                    CommandMode::NoView
                } else {
                    CommandMode::View
                };
                assert_eq!(command.mode, mode);
                let icon = folder.path().join("icon.png");
                assert_eq!(
                    icons::png_size(&icon),
                    Some((icons::PUBLISHED_ICON_SIZE, icons::PUBLISHED_ICON_SIZE))
                );
                // The manifest carries a $schema (an editor's, not Pane's:
                // Pane ignores fields it does not know, so it is no
                // problem), and the only problem is the component, which
                // an author builds.
                let pane = fs::read_to_string(folder.path().join(MANIFEST_FILE)).unwrap();
                assert!(pane.contains("$schema"), "{pane}");
                let report = check::check_package(folder.path());
                assert!(report.warnings.is_empty(), "{:?}", report.warnings);
                assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
                assert_eq!(report.errors[0].id, check::MANIFEST);
            }
        }
    }

    #[test]
    fn a_name_becomes_the_spellings_a_package_uses() {
        let name = Name::parse("Word Count").unwrap();
        assert_eq!(name.title, "Word Count");
        assert_eq!(name.package, "word-count");
        assert_eq!(name.crate_name(), "word_count");
        assert_eq!(name.type_name(), "WordCount");
        // What the author wrote keeps its shape in Title Case, with the
        // two characters that would break the sources' string literals
        // gone, and the package name takes its letters and digits.
        assert_eq!(Name::parse("  say  hello  ").unwrap().title, "Say Hello");
        assert_eq!(Name::parse("notes!").unwrap().package, "notes");
        assert_eq!(Name::parse("Say \"Hello\"").unwrap().title, "Say Hello");
        for refused in [Name::parse("!!!"), Name::parse(""), Name::parse("42 Things")] {
            assert!(refused.is_err());
        }
    }

    #[test]
    fn an_existing_folder_is_written_into_only_while_empty() {
        let name = Name::parse("Word Count").unwrap();
        let folder = tempfile::tempdir().unwrap();
        scaffold(folder.path(), &name, Language::TypeScript, Kind::List).unwrap();
        let refused = scaffold(folder.path(), &name, Language::Rust, Kind::List);
        assert!(refused.unwrap_err().contains("is not empty"));
        // An empty folder is written into; a file is refused.
        let empty = tempfile::tempdir().unwrap();
        scaffold(empty.path(), &name, Language::Rust, Kind::List).unwrap();
        assert!(empty.path().join("Cargo.toml").is_file());
        let file = tempfile::tempdir().unwrap();
        let file = file.path().join("word-count");
        fs::write(&file, "a file").unwrap();
        let refused = scaffold(&file, &name, Language::Rust, Kind::List);
        assert!(refused.unwrap_err().contains("is a file"));
    }

    /// Adds a command to a scaffolded package of `language` from its
    /// `list` template, and answers the folder.
    fn with_added_command(language: Language) -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        let name = Name::parse("Word Count").unwrap();
        scaffold(folder.path(), &name, language, Kind::List).unwrap();
        let note = NewCommand::parse(Some("note"), Some("Note")).unwrap();
        add_command(folder.path(), &note, Kind::Detail).unwrap();
        folder
    }

    #[test]
    fn add_command_adds_a_command_the_package_still_parses() {
        for language in [Language::Rust, Language::TypeScript] {
            let folder = with_added_command(language);
            let (manifest, _) = Manifest::read_parsed(folder.path()).unwrap();
            assert_eq!(manifest.commands.len(), 2);
            let note = &manifest.commands[1];
            assert_eq!(note.id, "note");
            assert_eq!(note.title, "Note");
            assert!(note.takes_query, "the detail template takes a query");
            // The command is served by the component the package's own
            // command is.
            assert_eq!(note.component, manifest.commands[0].component);
            // The command's file is the template's, with the command's
            // spellings in it.
            let (path, contents) = command_file(language, Kind::Detail);
            let path = folder.path().join(&path);
            let expected = contents.replace("__TITLE__", "Note");
            let expected = expected.replace("__NAME__", "note");
            assert_eq!(fs::read_to_string(&path).unwrap(), expected);
            // The entry file's dispatch calls it.
            let entry = folder.path().join(entry_file(language));
            let entry = fs::read_to_string(&entry).unwrap();
            match language {
                Language::Rust => {
                    assert!(entry.contains("mod note;"), "{entry}");
                    assert!(entry.contains("\"note\" => note::render().await,"), "{entry}");
                }
                Language::TypeScript => {
                    assert!(entry.contains("import * as note from \"./note\";"), "{entry}");
                    let arm = "case \"note\": return note.render(launch);";
                    assert!(entry.contains(arm), "{entry}");
                }
            }
            // A second command of the same name is refused.
            let again = NewCommand::parse(Some("note"), None).unwrap();
            let refused = add_command(folder.path(), &again, Kind::Form);
            assert!(refused.unwrap_err().contains("already has a command named `note`"));
        }
    }

    #[test]
    fn add_command_adds_a_no_view_command_to_the_run_dispatch() {
        let folder = tempfile::tempdir().unwrap();
        let name = Name::parse("Word Count").unwrap();
        scaffold(folder.path(), &name, Language::TypeScript, Kind::List).unwrap();
        let tick = NewCommand::parse(None, Some("Tick Tock")).unwrap();
        add_command(folder.path(), &tick, Kind::NoView).unwrap();
        let entry = folder.path().join("src/index.ts");
        let entry = fs::read_to_string(&entry).unwrap();
        let import = "import * as tickTock from \"./tick-tock\";";
        assert!(entry.contains(import), "{entry}");
        let arm = "case \"tick-tock\": return tickTock.run(_launch);";
        assert!(entry.contains(arm), "{entry}");
        let (manifest, _) = Manifest::read_parsed(folder.path()).unwrap();
        assert_eq!(manifest.commands[1].mode, CommandMode::NoView);
    }

    #[test]
    fn add_command_adds_a_form_command_to_the_form_dispatch() {
        let folder = tempfile::tempdir().unwrap();
        let name = Name::parse("Word Count").unwrap();
        scaffold(folder.path(), &name, Language::Rust, Kind::List).unwrap();
        let ask = NewCommand::parse(Some("ask"), None).unwrap();
        add_command(folder.path(), &ask, Kind::Form).unwrap();
        let entry = folder.path().join("src/lib.rs");
        let entry = fs::read_to_string(&entry).unwrap();
        assert!(entry.contains("mod ask;"), "{entry}");
        assert!(entry.contains("\"ask\" => ask::render().await,"), "{entry}");
        let arm = "\"ask\" => ask::submit_form(item_id, _values).await,";
        assert!(entry.contains(arm), "{entry}");
    }

    #[test]
    fn add_command_refuses_a_package_whose_markers_are_gone() {
        let folder = tempfile::tempdir().unwrap();
        let name = Name::parse("Word Count").unwrap();
        scaffold(folder.path(), &name, Language::TypeScript, Kind::List).unwrap();
        let entry = folder.path().join("src/index.ts");
        let source = fs::read_to_string(&entry).unwrap();
        let marker = "pane-ext new command adds a view command's arm here.\n";
        let without = source.replace(marker, "");
        fs::write(&entry, without).unwrap();
        let note = NewCommand::parse(Some("note"), None).unwrap();
        let refused = add_command(folder.path(), &note, Kind::List);
        assert!(refused.unwrap_err().contains("no longer carries pane-ext's marker"));
        // Nothing was written: a refused command leaves the package as it
        // was.
        assert!(!folder.path().join("src/note.ts").exists());
    }

    #[test]
    fn add_command_refuses_a_package_without_the_templates_entry_file() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(
            folder.path().join(MANIFEST_FILE),
            r#"{ "manifestVersion": 1, "title": "Hello", "apiVersion": "0.1",
                 "commands": [ { "id": "hello", "title": "Hello", "component": "hello.wasm" } ] }"#,
        )
        .unwrap();
        fs::write(folder.path().join("package.json"), "{}").unwrap();
        let note = NewCommand::parse(Some("note"), None).unwrap();
        let refused = add_command(folder.path(), &note, Kind::List);
        assert!(refused.unwrap_err().contains("neither the Rust nor the TypeScript entry"));
    }

    #[test]
    fn new_commands_are_named_by_their_ids_or_titles() {
        let expected = NewCommand {
            id: "word-count".into(),
            title: "Word Count".into(),
        };
        let by_id = NewCommand::parse(Some("word-count"), None).unwrap();
        assert_eq!(by_id, expected);
        assert_eq!(NewCommand::parse(None, Some("Word Count")).unwrap(), expected);
        let named = NewCommand::parse(Some("note"), Some("Say Hello")).unwrap();
        assert_eq!(named.title, "Say Hello");
        assert!(NewCommand::parse(None, None).is_err());
        assert!(NewCommand::parse(None, Some("!!!")).is_err());
        assert!(NewCommand::parse(Some("!!!"), None).is_err());
    }

    #[test]
    fn the_choices_parse_as_the_flags_spell_them() {
        assert_eq!(Language::parse("rust"), Some(Language::Rust));
        assert_eq!(Language::parse("TypeScript"), Some(Language::TypeScript));
        assert_eq!(Language::parse("ts"), Some(Language::TypeScript));
        assert_eq!(Language::parse("javascript"), None);
        assert_eq!(Kind::parse("list"), Some(Kind::List));
        assert_eq!(Kind::parse("no-view"), Some(Kind::NoView));
        assert_eq!(Kind::parse("NoView"), None);
        assert_eq!(Kind::parse("table"), None);
    }
}
