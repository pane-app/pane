//! The WIT guard (#319, ADR 0046): no change of shape ships under an
//! extension API version that has already shipped. The guard compares
//! `wit/` with the WIT of the released API — the last release tag
//! (`vX.Y.Z`, the greatest one being the last release), and until the
//! repository has one, the WIT as of the last commit that changed the
//! `pane:extension` package's version, its introduction counting as the
//! first such change — and fails when the change is breaking and the
//! version did not move, naming what broke and what must move. A change
//! with the version moved passes: the guard refuses silent breaks, not
//! version bumps. A purely additive change under the same version
//! passes too. `ci-lints` runs it beside the schema and the one
//! extension API number; the lints jobs' checkouts fetch the whole
//! history its baseline walk reads.
//!
//! The comparison is structural, over name-keyed snapshots of both
//! sides' parsed packages: interfaces, worlds, functions and types by
//! the names the WIT gives them, never by the parser's internal ids.
//! Includes are compared as elaborated — wit-parser elaborates them on
//! resolution — so a world's contract is the sum of what it imports and
//! exports, not the includes it was written with. The version this
//! package's own interfaces are named by is never part of a name: a
//! move of the version must not look like a rename of everything.
//!
//! Breaking, by the specification's rule: removing or renaming an
//! interface, function, type, field, case or flag (a rename is a removal
//! and an addition, and the removal half is what breaks —
//! conservative); changing a parameter or result type, a function's
//! parameter list, or its kind (parameter NAMES are not types: a
//! name-only change is not breaking); reordering a record's fields, a
//! variant's cases, an enum's cases or a flags' flags — the last two
//! because their order is what each case's and flag's value IS; adding
//! a field to an existing record; a world dropping or renaming an
//! import or export key; a foreign import's identity changing
//! (`wasi:…@…`). Not breaking: adding a function, a type, an interface,
//! a world, a world import or export, a variant case, an enum case or a
//! flag. The guard is a net over the WIT's structure, not a proof: a
//! change in what a call does — the rule's behavioural class — is
//! outside it, and stays with the type check that refuses a component
//! built for another shape.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use wit_parser::{
    Function, FunctionKind, Handle, InterfaceId, PackageId, Resolve, Type, TypeDefKind, TypeId,
    TypeOwner, WorldItem, WorldKey,
};

/// Checks that the extension API's WIT may still carry its version: the
/// changes to `wit/` since the released API are additive, or the version
/// moved. `ci-lints` runs this in the shape of the checks beside it: a
/// failing message names what broke and what to do, passing prints one
/// line.
pub(crate) fn check() -> Result<(), String> {
    let root = crate::root();
    // This run's scratch: the baseline's WIT, written out of the
    // repository's history, and git's own home, so no configuration of
    // the developer's changes what the guard reads.
    let scratch = std::env::temp_dir().join(format!("pane-wit-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let result = guard(&root, &scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// The guard's work, under `scratch`: the baseline, both sides parsed,
/// and the verdict.
fn guard(root: &Path, scratch: &Path) -> Result<(), String> {
    let baseline = baseline(root, scratch)?;
    let mut current = Resolve::new();
    current
        .push_dir(root.join("wit"))
        .map_err(|error| format!("wit/ does not parse: {}", current.render_error(&error)))?;
    let current = snapshot_of(&current)?;
    let old = baseline_snapshot(root, scratch, &baseline)?;
    match verdict(&old, &current) {
        Verdict::Moved(version) => {
            println!(
                "xtask: the extension API version moved to {version}, so whatever changed \
                 in wit/ ships under a new number"
            );
        }
        Verdict::Additive(version) => {
            println!(
                "xtask: the changes to wit/ since {} are additive: pane:extension@{version} \
                 kept its shape",
                baseline.label
            );
        }
        Verdict::Breaking(problems) => return Err(failure(&problems)),
    }
    Ok(())
}

/// The released API the guard compares `wit/` with, and how the baseline
/// is named in messages.
struct Baseline {
    /// The ref the WIT is read at.
    reference: String,
    /// How the baseline is named in messages.
    label: String,
}

/// The released API: the WIT of the last release tag, and until the
/// repository has one, the WIT as of the last commit that changed the
/// `pane:extension` package's version.
fn baseline(root: &Path, scratch: &Path) -> Result<Baseline, String> {
    // A release tag, when the repository holds one: the released API is
    // the WIT of Pane's last release.
    let tags = capture(git(root, scratch).arg("tag").arg("--list"))?;
    let tags: Vec<String> = tags.lines().map(str::to_owned).collect();
    if let Some(tag) = last_release_tag(&tags) {
        return Ok(Baseline {
            reference: tag.clone(),
            label: format!("the release tag {tag}"),
        });
    }
    // Until one does: the last commit that changed the WIT package's
    // version, its introduction counting as the first such change. The
    // commits that touched `wit/`, newest first, each with its first
    // parent (a parent that never touched `wit/` is elided, so a commit
    // that introduced the tree has none): the versions both declare say
    // where a version was declared. The walk is over the full history:
    // the default simplification would prune a merge that is treesame
    // with one of its sides for `wit/`, and main's adoption of a
    // version's content is exactly such a merge — without the merges
    // the walk linearizes through the branch that changed the content,
    // and the adoption itself reads as post-declaration drift. Full
    // history keeps the adoption merges, and the parents it shows are
    // the nearest ancestors that touched `wit/`, so the baseline is the
    // last commit that changed the version on mainline: a merge
    // resolution that carries a version's content is its adoption, and
    // whatever changes `wit/` on mainline after it is still drift the
    // guard flags.
    let history = capture(
        git(root, scratch)
            .args(["rev-list", "--full-history", "--parents", "HEAD", "--", "wit/"]),
    )?;
    // The version sets are cached: a commit is its successor's parent.
    let mut sets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut commits = Vec::new();
    for line in history.lines() {
        let mut ids = line.split_whitespace();
        let Some(sha) = ids.next() else {
            continue;
        };
        let versions = version_set(root, scratch, sha, &mut sets)?;
        let parent = match ids.next() {
            Some(parent) => version_set(root, scratch, parent, &mut sets)?,
            // No parent in `wit/`'s history: the introduction, an empty
            // tree before it.
            None => BTreeSet::new(),
        };
        commits.push((sha.to_owned(), versions, parent));
    }
    let Some(sha) = baseline_commit(&commits) else {
        return Err(
            "the WIT guard found no commit that touched wit/, so it cannot pick the \
             baseline of the extension API"
                .to_owned(),
        );
    };
    let declared = sets
        .get(&sha)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");
    let short: String = sha.chars().take(9).collect();
    Ok(Baseline {
        reference: sha,
        label: format!("the commit {short} that declared pane:extension@{declared}"),
    })
}

/// The versions the `wit/` files at `sha` declare in their `package
/// pane:extension@…` lines, cached in `sets`: the empty set before the
/// package was introduced. One `git grep` reads them all.
fn version_set(
    root: &Path,
    scratch: &Path,
    sha: &str,
    sets: &mut BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeSet<String>, String> {
    if let Some(set) = sets.get(sha) {
        return Ok(set.clone());
    }
    let output = git(root, scratch)
        .arg("grep")
        .arg("-h")
        .arg("package pane:extension@")
        .arg(sha)
        .arg("--")
        // The package's own files only: `deps/` holds WASI's WIT, and a
        // `previous/` folder would hold the previous version's world.
        .arg(":(glob)wit/*.wit")
        .output()
        .map_err(|error| format!("failed to start git grep at {sha}: {error}"))?;
    // git grep exits 1 when nothing matches: a tree with no declaration,
    // not a failure.
    let text = match output.status.code() {
        Some(0) | Some(1) => String::from_utf8_lossy(&output.stdout).into_owned(),
        _ => {
            return Err(format!(
                "git grep for the package declarations at {sha} failed with {}",
                output.status
            ));
        }
    };
    let set = versions_of(&text);
    sets.insert(sha.to_owned(), set.clone());
    Ok(set)
}

/// The versions the `package pane:extension@…` declarations name, from
/// what `git grep` prints for them: the set a commit's `wit/` declares,
/// compared with its parent's to find where a version was declared.
fn versions_of(grep: &str) -> BTreeSet<String> {
    let mut versions = BTreeSet::new();
    for line in grep.lines() {
        let Some(rest) = line.trim_start().strip_prefix("package pane:extension@") else {
            continue;
        };
        let version = rest.split(';').next().unwrap_or_default().trim();
        if !version.is_empty() {
            versions.insert(version.to_owned());
        }
    }
    versions
}

/// The repository's last release tag, from every tag it holds: the
/// greatest `vX.Y.Z` — Pane's release tags, when it has them. A tag of
/// another shape (a moving tip's, a pre-release's) is not a release.
fn last_release_tag(tags: &[String]) -> Option<String> {
    tags.iter()
        .filter_map(|tag| {
            let numbers = crate::api_versions::parse_version(tag.strip_prefix('v')?)?;
            Some((numbers, tag))
        })
        .max()
        .map(|(_, tag)| tag.clone())
}

/// The commit whose WIT is the baseline, from the commits that touched
/// `wit/`, newest first, each with the versions its files declare and
/// the versions its first parent's files declare: the newest commit
/// whose set differs from its parent's — the point the current version
/// was declared, its introduction counting as the first such change. A
/// walk that finds no such commit (which cannot happen: the oldest
/// commit introduced the tree against an empty parent) takes the oldest
/// commit, the most conservative baseline.
fn baseline_commit(commits: &[(String, BTreeSet<String>, BTreeSet<String>)]) -> Option<String> {
    for (sha, versions, parent) in commits {
        if versions != parent {
            return Some(sha.clone());
        }
    }
    commits.last().map(|(sha, _, _)| sha.clone())
}

/// The baseline's `wit/` folder, written under `scratch` out of the
/// repository's history: every file under `wit/` at the baseline, as it
/// was then, so the same parser reads it that reads `wit/` now.
fn baseline_wit(root: &Path, scratch: &Path, baseline: &Baseline) -> Result<PathBuf, String> {
    let tree = capture(
        git(root, scratch)
            .args(["ls-tree", "-r", "--name-only"])
            .arg(&baseline.reference)
            .args(["--", "wit/"]),
    )?;
    if tree.is_empty() {
        return Err(format!(
            "{} holds no WIT to compare with: the extension API's WIT came later",
            baseline.label
        ));
    }
    let wit = scratch.join("wit");
    for path in tree.lines() {
        let contents = file_at(root, scratch, &baseline.reference, path)?;
        let to = wit.join(path.strip_prefix("wit/").unwrap_or(path));
        if let Some(folder) = to.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|error| format!("{} cannot be created: {error}", folder.display()))?;
        }
        std::fs::write(&to, contents)
            .map_err(|error| format!("{} cannot be written: {error}", to.display()))?;
    }
    Ok(wit)
}

/// One file's contents at `reference`, as git holds them.
fn file_at(root: &Path, scratch: &Path, reference: &str, path: &str) -> Result<String, String> {
    let output = git(root, scratch)
        .arg("show")
        .arg(format!("{reference}:{path}"))
        .output()
        .map_err(|error| format!("failed to start git show for {path}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git show {reference}:{path} failed with {}",
            output.status
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The baseline's snapshot: its `wit/` tree written out of the history
/// and parsed the same way `wit/` is, then read back from `scratch`.
fn baseline_snapshot(root: &Path, scratch: &Path, baseline: &Baseline) -> Result<Snapshot, String> {
    let wit = baseline_wit(root, scratch, baseline)?;
    let mut resolve = Resolve::new();
    let snapshot = match resolve.push_dir(&wit) {
        Ok(_) => snapshot_of(&resolve),
        Err(error) => Err(format!(
            "{} holds WIT that does not parse: {}",
            baseline.label,
            resolve.render_error(&error)
        )),
    };
    let _ = std::fs::remove_dir_all(&wit);
    snapshot
}

/// `git` with an environment of its own, run in the repository's root:
/// a read-only guard has no use for a developer's git configuration,
/// and none of it may change what is read.
fn git(root: &Path, scratch: &Path) -> Command {
    let mut command = Command::new("git");
    command.env_clear();
    for kept in ["PATH", "SYSTEMROOT", "TMP", "TEMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(kept) {
            command.env(kept, value);
        }
    }
    command
        .env("HOME", scratch)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", scratch.join("gitconfig"))
        .current_dir(root);
    command
}

/// What `command` prints on standard output, trimmed.
fn capture(command: &mut Command) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("failed to start {command:?}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!("{command:?} failed with {}", output.status))
    }
}

/// The name-keyed shape of one side of the comparison: the
/// `pane:extension` package's version, interfaces and worlds, every map
/// keyed by name so two snapshots compare by what the WIT names, never
/// by the order the parser met its items in.
#[derive(Debug, PartialEq)]
struct Snapshot {
    /// The version the package declares, `X.Y.Z`, or none when it
    /// declares no version.
    version: Option<String>,
    /// Every interface of the package, by name.
    interfaces: BTreeMap<String, InterfaceSnap>,
    /// Every world of the package, by name.
    worlds: BTreeMap<String, WorldSnap>,
}

/// One interface's shape: its named types and its functions, by name.
#[derive(Debug, PartialEq)]
struct InterfaceSnap {
    types: BTreeMap<String, TypeSnap>,
    functions: BTreeMap<String, FunctionSnap>,
}

/// One world's shape: its imports and exports, by the key the WIT holds
/// them under.
#[derive(Debug, PartialEq)]
struct WorldSnap {
    imports: BTreeMap<String, WorldItemSnap>,
    exports: BTreeMap<String, WorldItemSnap>,
}

/// What one world import or export holds.
#[derive(Debug, PartialEq)]
enum WorldItemSnap {
    /// An interface, by the path that names it: `pane:extension/command`
    /// for this package's own — the version is the snapshot's own, and
    /// never part of the identity — the versioned
    /// `wasi:cli/stdout@0.3.0` for a foreign one. What the interface
    /// holds is compared by its own section, or not at all for a foreign
    /// one: its identity is the contract here.
    Interface(String),
    /// An inline interface, its shape held here: it has no name of its
    /// own and no section to be compared by.
    Inline(InterfaceSnap),
    /// A function the world imports or exports directly.
    Function(FunctionSnap),
    /// A type the world exports.
    Type(TypeSnap),
}

/// A function's shape: its kind, its parameters' types in order, and
/// its result. The parameters' names are kept for messages only: names
/// are not types.
#[derive(Debug, PartialEq)]
struct FunctionSnap {
    /// The function's kind, as messages name it: a method, an async
    /// function, a constructor. Resource functions carry their resource
    /// in the name they are keyed by.
    kind: String,
    /// The parameters, in order.
    params: Vec<(String, TypeSnap)>,
    /// The result's type, when it has one.
    result: Option<TypeSnap>,
}

/// A type's shape as the snapshot holds it: a primitive by its name, a
/// named type of this package by the path that names it (its own
/// section compares its shape), and anonymous composites of other
/// snapshot types.
#[derive(Debug, PartialEq)]
enum TypeSnap {
    /// A primitive, by its name: `u8`, `string`, …
    Primitive(&'static str),
    /// A named type, by the path that names it.
    Named(String),
    List(Box<TypeSnap>),
    Option(Box<TypeSnap>),
    Result(Option<Box<TypeSnap>>, Option<Box<TypeSnap>>),
    Tuple(Vec<TypeSnap>),
    Map(Box<TypeSnap>, Box<TypeSnap>),
    /// `own<t>` or `borrow<t>` of a resource.
    Handle(bool, Box<TypeSnap>),
    /// A record's fields, in order.
    Record(Vec<(String, TypeSnap)>),
    /// A variant's cases, in order, with each case's payload.
    Variant(Vec<(String, Option<TypeSnap>)>),
    /// An enum's cases, in order.
    Enum(Vec<String>),
    /// A flags' flags, in order.
    Flags(Vec<String>),
    /// A resource: its functions are the interface's, keyed by their
    /// mangled names.
    Resource,
    /// A stream or future, with its payload when it has one.
    Stream(Option<Box<TypeSnap>>),
    Future(Option<Box<TypeSnap>>),
    /// A fixed-length list.
    FixedList(Box<TypeSnap>, u32),
    /// A type of unknown structure, which a resolved package cannot
    /// hold.
    Unknown,
}

/// The snapshot of the `pane:extension` package in `resolve`: the one
/// package the guard is of — exactly one, whatever version it declares
/// (`api_versions` has already checked that `wit/`'s files agree on
/// one).
fn snapshot_of(resolve: &Resolve) -> Result<Snapshot, String> {
    let mut packages = resolve
        .package_names
        .iter()
        .filter(|(name, _)| name.namespace == "pane" && name.name == "extension");
    let (name, package) = packages
        .next()
        .ok_or("the WIT holds no `pane:extension` package for the guard to compare")?;
    if packages.next().is_some() {
        return Err("the WIT holds more than one `pane:extension` package".to_owned());
    }
    let walker = Snapshotter {
        resolve,
        package: *package,
        prefix: format!("{}:{}", name.namespace, name.name),
        inline: inline_interfaces(resolve, *package),
    };
    Ok(Snapshot {
        version: name.version.as_ref().map(|version| version.to_string()),
        interfaces: walker.interfaces(),
        worlds: walker.worlds(),
    })
}

/// Each inline interface of the package's worlds, by its arena index: no
/// name of its own, it is named by the world and the key it sits under —
/// the path of the types it owns.
fn inline_interfaces(resolve: &Resolve, package: PackageId) -> BTreeMap<usize, String> {
    let mut inline = BTreeMap::new();
    for (_, world_id) in resolve.packages[package].worlds.iter() {
        let world = &resolve.worlds[*world_id];
        for (key, item) in world.imports.iter().chain(world.exports.iter()) {
            // The key of an inline interface is its label.
            if let (WorldKey::Name(label), WorldItem::Interface { id, .. }) = (key, item)
                && resolve.interfaces[*id].name.is_none()
            {
                inline.insert(id.index(), format!("world:{}/{}", world.name, label));
            }
        }
    }
    inline
}

/// The walk over one `Resolve` that builds its snapshot: the package it
/// is of, and the paths that name its interfaces and types.
struct Snapshotter<'a> {
    resolve: &'a Resolve,
    /// The `pane:extension` package the snapshot is of.
    package: PackageId,
    /// The package's name without its version, `pane:extension`: the
    /// prefix of every path that names its interfaces.
    prefix: String,
    /// Each inline interface of the package's worlds, by its arena
    /// index.
    inline: BTreeMap<usize, String>,
}

impl<'a> Snapshotter<'a> {
    /// The package's interfaces, by name.
    fn interfaces(&self) -> BTreeMap<String, InterfaceSnap> {
        let package = &self.resolve.packages[self.package];
        let mut interfaces = BTreeMap::new();
        for (name, id) in package.interfaces.iter() {
            interfaces.insert(name.clone(), self.interface(*id));
        }
        interfaces
    }

    /// One interface's shape: its named types and its functions, by
    /// name. Resource functions are keyed by wit-parser's mangled names
    /// (`[method]r.f`), which say what their name already does.
    fn interface(&self, id: InterfaceId) -> InterfaceSnap {
        let interface = &self.resolve.interfaces[id];
        let mut types = BTreeMap::new();
        for (name, type_id) in interface.types.iter() {
            types.insert(name.clone(), self.type_shape(*type_id));
        }
        let mut functions = BTreeMap::new();
        for (name, function) in interface.functions.iter() {
            functions.insert(name.clone(), self.function(function));
        }
        InterfaceSnap { types, functions }
    }

    /// The package's worlds, by name: every import and export under the
    /// key the WIT holds it by, its contents elaborated.
    fn worlds(&self) -> BTreeMap<String, WorldSnap> {
        let package = &self.resolve.packages[self.package];
        let mut worlds = BTreeMap::new();
        for (name, id) in package.worlds.iter() {
            let world = &self.resolve.worlds[*id];
            let mut imports = BTreeMap::new();
            let mut exports = BTreeMap::new();
            for (key, item) in world.imports.iter() {
                imports.insert(self.key(key), self.item(item));
            }
            for (key, item) in world.exports.iter() {
                exports.insert(self.key(key), self.item(item));
            }
            worlds.insert(name.clone(), WorldSnap { imports, exports });
        }
        worlds
    }

    /// The name a world holds an item under: a plain label, or the path
    /// of the interface it names.
    fn key(&self, key: &WorldKey) -> String {
        match key {
            WorldKey::Name(name) => name.clone(),
            WorldKey::Interface(id) => self.interface_path(*id),
        }
    }

    /// The path of a named interface: `pane:extension/command` in this
    /// package, `wasi:cli/stdout@0.3.0` in another — this package's own
    /// version is never part of the path (the version is what the guard
    /// asks about, and its move must not rename every reference), a
    /// foreign package's is, as its identity.
    fn interface_path(&self, id: InterfaceId) -> String {
        let interface = &self.resolve.interfaces[id];
        let package = interface
            .package
            .expect("a resolved interface belongs to a package");
        let mut path = if package == self.package {
            self.prefix.clone()
        } else {
            let name = &self.resolve.packages[package].name;
            format!("{}:{}", name.namespace, name.name)
        };
        if let Some(name) = &interface.name {
            path.push('/');
            path.push_str(name);
        }
        if package != self.package
            && let Some(version) = &self.resolve.packages[package].name.version
        {
            path.push('@');
            path.push_str(&version.to_string());
        }
        path
    }

    /// What one world import or export holds: an interface by its path
    /// (an inline one by its shape, held here), a function, or a type.
    fn item(&self, item: &WorldItem) -> WorldItemSnap {
        match item {
            WorldItem::Interface { id, .. } => {
                if self.resolve.interfaces[*id].name.is_some() {
                    WorldItemSnap::Interface(self.interface_path(*id))
                } else {
                    WorldItemSnap::Inline(self.interface(*id))
                }
            }
            WorldItem::Function(function) => WorldItemSnap::Function(self.function(function)),
            WorldItem::Type { id, .. } => WorldItemSnap::Type(self.type_shape(*id)),
        }
    }

    /// A function's shape: its kind, its parameters' types in order, and
    /// its result.
    fn function(&self, function: &Function) -> FunctionSnap {
        FunctionSnap {
            kind: self.kind(&function.kind),
            params: function
                .params
                .iter()
                .map(|param| (param.name.clone(), self.type_of(&param.ty)))
                .collect(),
            result: function.result.as_ref().map(|ty| self.type_of(ty)),
        }
    }

    /// A function's kind, as messages name it: the role and whether it
    /// is async — the resource a resource function belongs to is in the
    /// name it is keyed by, so a move between resources is a removal of
    /// the one key and an addition of the other.
    fn kind(&self, kind: &FunctionKind) -> String {
        let role = match kind {
            FunctionKind::Freestanding => "a function",
            FunctionKind::AsyncFreestanding => "an async function",
            FunctionKind::Method(_) => "a method",
            FunctionKind::AsyncMethod(_) => "an async method",
            FunctionKind::Static(_) => "a static function",
            FunctionKind::AsyncStatic(_) => "an async static function",
            FunctionKind::Constructor(_) => "a constructor",
        };
        role.to_owned()
    }

    /// A type reference's snapshot: a primitive, a named type by the
    /// path that names it (its own section compares its shape), or an
    /// anonymous composite's shape.
    fn type_of(&self, ty: &Type) -> TypeSnap {
        match ty {
            Type::Bool => TypeSnap::Primitive("bool"),
            Type::U8 => TypeSnap::Primitive("u8"),
            Type::U16 => TypeSnap::Primitive("u16"),
            Type::U32 => TypeSnap::Primitive("u32"),
            Type::U64 => TypeSnap::Primitive("u64"),
            Type::S8 => TypeSnap::Primitive("s8"),
            Type::S16 => TypeSnap::Primitive("s16"),
            Type::S32 => TypeSnap::Primitive("s32"),
            Type::S64 => TypeSnap::Primitive("s64"),
            Type::F32 => TypeSnap::Primitive("f32"),
            Type::F64 => TypeSnap::Primitive("f64"),
            Type::Char => TypeSnap::Primitive("char"),
            Type::String => TypeSnap::Primitive("string"),
            Type::ErrorContext => TypeSnap::Primitive("error-context"),
            Type::Id(id) => {
                if self.resolve.types[*id].name.is_some() {
                    TypeSnap::Named(self.type_path(*id))
                } else {
                    self.type_shape(*id)
                }
            }
        }
    }

    /// The path of a named type: the interface or world that owns it,
    /// and the type's name — `pane:extension/commands.launch-record`
    /// here, `wasi:cli/stdout@0.3.0.request` in a foreign package.
    fn type_path(&self, id: TypeId) -> String {
        let typedef = &self.resolve.types[id];
        let name = typedef.name.clone().unwrap_or_default();
        match typedef.owner {
            TypeOwner::Interface(interface) => {
                if self.resolve.interfaces[interface].name.is_some() {
                    format!("{}.{}", self.interface_path(interface), name)
                } else {
                    // An inline interface: named by the world and key it
                    // sits under.
                    let path = self
                        .inline
                        .get(&interface.index())
                        .cloned()
                        .unwrap_or_else(|| "an inline interface".to_owned());
                    format!("{path}.{name}")
                }
            }
            TypeOwner::World(world) => {
                format!("world:{}.{}", self.resolve.worlds[world].name, name)
            }
            // A named type without an owner, which a resolved package
            // cannot hold: the name alone.
            TypeOwner::None => name,
        }
    }

    /// A named type's shape: its structure, with every named type inside
    /// it by the path that names it. An alias's shape is its target's.
    fn type_shape(&self, id: TypeId) -> TypeSnap {
        let typedef = &self.resolve.types[id];
        match &typedef.kind {
            TypeDefKind::Record(record) => TypeSnap::Record(
                record
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), self.type_of(&field.ty)))
                    .collect(),
            ),
            TypeDefKind::Variant(variant) => TypeSnap::Variant(
                variant
                    .cases
                    .iter()
                    .map(|case| {
                        (
                            case.name.clone(),
                            case.ty.as_ref().map(|ty| self.type_of(ty)),
                        )
                    })
                    .collect(),
            ),
            TypeDefKind::Enum(enum_) => {
                TypeSnap::Enum(enum_.cases.iter().map(|case| case.name.clone()).collect())
            }
            TypeDefKind::Flags(flags) => {
                TypeSnap::Flags(flags.flags.iter().map(|flag| flag.name.clone()).collect())
            }
            TypeDefKind::Resource => TypeSnap::Resource,
            TypeDefKind::Handle(handle) => {
                let (own, inner) = match handle {
                    Handle::Own(id) => (true, Type::Id(*id)),
                    Handle::Borrow(id) => (false, Type::Id(*id)),
                };
                TypeSnap::Handle(own, Box::new(self.type_of(&inner)))
            }
            TypeDefKind::Tuple(tuple) => {
                TypeSnap::Tuple(tuple.types.iter().map(|ty| self.type_of(ty)).collect())
            }
            TypeDefKind::Option(ty) => TypeSnap::Option(Box::new(self.type_of(ty))),
            TypeDefKind::Result(result) => TypeSnap::Result(
                result.ok.as_ref().map(|ty| Box::new(self.type_of(ty))),
                result.err.as_ref().map(|ty| Box::new(self.type_of(ty))),
            ),
            TypeDefKind::List(ty) => TypeSnap::List(Box::new(self.type_of(ty))),
            TypeDefKind::Map(key, value) => {
                TypeSnap::Map(Box::new(self.type_of(key)), Box::new(self.type_of(value)))
            }
            TypeDefKind::FixedLengthList(ty, length) => {
                TypeSnap::FixedList(Box::new(self.type_of(ty)), *length)
            }
            TypeDefKind::Future(ty) => {
                TypeSnap::Future(ty.as_ref().map(|ty| Box::new(self.type_of(ty))))
            }
            TypeDefKind::Stream(ty) => {
                TypeSnap::Stream(ty.as_ref().map(|ty| Box::new(self.type_of(ty))))
            }
            // An alias: its target's snapshot.
            TypeDefKind::Type(ty) => self.type_of(ty),
            TypeDefKind::Unknown => TypeSnap::Unknown,
        }
    }
}

/// The guard's verdict for the WIT `new` against the baseline `old`.
#[derive(Debug, PartialEq)]
enum Verdict {
    /// The version moved: whatever changed, it did not ship under a
    /// number that had already shipped. The version it moved to.
    Moved(String),
    /// The changes are additive under the same version. The version.
    Additive(String),
    /// The problems that must move the version.
    Breaking(Vec<String>),
}

/// The verdict: a version that moved is itself the answer, whatever
/// changed with it; the same version may carry additive changes only; a
/// package that declares no version cannot be judged at all.
fn verdict(old: &Snapshot, new: &Snapshot) -> Verdict {
    let (Some(old_version), Some(new_version)) = (&old.version, &new.version) else {
        return Verdict::Breaking(vec![
            "the `pane:extension` WIT package declares no version, so the guard cannot tell \
             whether the extension API version moved"
                .to_owned(),
        ]);
    };
    if old_version != new_version {
        return Verdict::Moved(new_version.clone());
    }
    let problems = breaking_changes(old, new);
    if problems.is_empty() {
        Verdict::Additive(old_version.clone())
    } else {
        Verdict::Breaking(problems)
    }
}

/// The breaking changes between the baseline's snapshot `old` and the
/// current one `new`, one line each, by the specification's rule (#319,
/// ADR 0046): removals and renames, changed types, added record fields,
/// and reorders. Additions are not here: they need no new version
/// before 1.0.
fn breaking_changes(old: &Snapshot, new: &Snapshot) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, old_interface) in &old.interfaces {
        match new.interfaces.get(name) {
            None => problems.push(format!("the interface `{name}` was removed")),
            Some(new_interface) => {
                let at = format!("the interface `{name}`");
                diff_types(
                    &at,
                    &old_interface.types,
                    &new_interface.types,
                    &mut problems,
                );
                diff_functions(
                    &at,
                    &old_interface.functions,
                    &new_interface.functions,
                    &mut problems,
                );
            }
        }
    }
    for (name, old_world) in &old.worlds {
        let Some(new_world) = new.worlds.get(name) else {
            problems.push(format!("the world `{name}` was removed"));
            continue;
        };
        diff_items(
            name,
            "import",
            "imports",
            &old_world.imports,
            &new_world.imports,
            &mut problems,
        );
        diff_items(
            name,
            "export",
            "exports",
            &old_world.exports,
            &new_world.exports,
            &mut problems,
        );
    }
    problems
}

/// One side's named types, by name: a removed type or a changed shape is
/// breaking; an added type is not.
fn diff_types(
    at: &str,
    old: &BTreeMap<String, TypeSnap>,
    new: &BTreeMap<String, TypeSnap>,
    problems: &mut Vec<String>,
) {
    for (name, old_type) in old {
        match new.get(name) {
            None => problems.push(format!("the type `{name}` was removed from {at}")),
            Some(new_type) => type_problem(at, name, old_type, new_type, problems),
        }
    }
}

/// One side's functions, by name: a removed function or a changed
/// signature is breaking; an added function is not.
fn diff_functions(
    at: &str,
    old: &BTreeMap<String, FunctionSnap>,
    new: &BTreeMap<String, FunctionSnap>,
    problems: &mut Vec<String>,
) {
    for (name, old_function) in old {
        match new.get(name) {
            None => problems.push(format!(
                "the function `{}` was removed from {at}",
                message_name(name)
            )),
            Some(new_function) => function_problem(at, name, old_function, new_function, problems),
        }
    }
}

/// One type's shape: a record's fields, a variant's cases, an enum's or
/// flags' members each by their own rule; any other change of shape is
/// one problem.
fn type_problem(at: &str, name: &str, old: &TypeSnap, new: &TypeSnap, problems: &mut Vec<String>) {
    match (old, new) {
        (TypeSnap::Record(old), TypeSnap::Record(new)) => {
            record_problem(at, name, old, new, problems);
        }
        (TypeSnap::Variant(old), TypeSnap::Variant(new)) => {
            variant_problem(at, name, old, new, problems);
        }
        (TypeSnap::Enum(old), TypeSnap::Enum(new)) => {
            member_problem("case", "enum", at, name, old, new, problems);
        }
        (TypeSnap::Flags(old), TypeSnap::Flags(new)) => {
            member_problem("flag", "flags", at, name, old, new, problems);
        }
        _ if old != new => {
            problems.push(format!("the type `{name}` in {at} changed: {old} → {new}"))
        }
        _ => {}
    }
}

/// A record's fields, in order: a removed, retyped, reordered or ADDED
/// field is breaking — the added field is the specification's one
/// addition that breaks: a record an extension built lacks it.
fn record_problem(
    at: &str,
    name: &str,
    old: &[(String, TypeSnap)],
    new: &[(String, TypeSnap)],
    problems: &mut Vec<String>,
) {
    for (field, old_type) in old {
        match new.iter().find(|(new_name, _)| new_name == field) {
            None => problems.push(format!(
                "the field `{field}` was removed from the record `{name}` in {at}"
            )),
            Some((_, new_type)) => {
                if old_type != new_type {
                    problems.push(format!(
                        "the field `{field}` of the record `{name}` in {at} changed type: \
                         {old_type} → {new_type}"
                    ));
                }
            }
        }
    }
    for (field, _) in new {
        if !old.iter().any(|(old_name, _)| old_name == field) {
            problems.push(format!(
                "the field `{field}` was added to the record `{name}` in {at}"
            ));
        }
    }
    // The same fields in another order: the order is part of the shape.
    let old_names: Vec<&String> = old.iter().map(|(name, _)| name).collect();
    let new_names: Vec<&String> = new.iter().map(|(name, _)| name).collect();
    if old_names != new_names
        && old_names.iter().all(|name| new_names.contains(name))
        && new_names.iter().all(|name| old_names.contains(name))
    {
        problems.push(format!(
            "the fields of the record `{name}` in {at} were reordered"
        ));
    }
}

/// A variant's cases, in order: a removed case, a changed payload or a
/// reorder is breaking; an ADDED case is not — additions are not
/// breaking, the specification's rule.
fn variant_problem(
    at: &str,
    name: &str,
    old: &[(String, Option<TypeSnap>)],
    new: &[(String, Option<TypeSnap>)],
    problems: &mut Vec<String>,
) {
    for (case, old_payload) in old {
        match new.iter().find(|(new_name, _)| new_name == case) {
            None => problems.push(format!(
                "the case `{case}` was removed from the variant `{name}` in {at}"
            )),
            Some((_, new_payload)) => {
                if old_payload != new_payload {
                    problems.push(format!(
                        "the case `{case}` of the variant `{name}` in {at} changed type: \
                         {} → {}",
                        payload(old_payload.as_ref()),
                        payload(new_payload.as_ref())
                    ));
                }
            }
        }
    }
    // The same cases in another order: the order is part of the shape.
    let old_names: Vec<&String> = old.iter().map(|(name, _)| name).collect();
    let new_names: Vec<&String> = new.iter().map(|(name, _)| name).collect();
    if old_names != new_names
        && old_names.iter().all(|name| new_names.contains(name))
        && new_names.iter().all(|name| old_names.contains(name))
    {
        problems.push(format!(
            "the cases of the variant `{name}` in {at} were reordered"
        ));
    }
}

/// An enum's cases or a flags' flags, in order: a removed member is
/// breaking, and so is a reorder — the order is what each case's
/// discriminant and each flag's bit IS. An added member is not.
fn member_problem(
    member: &str,
    kind: &str,
    at: &str,
    name: &str,
    old: &[String],
    new: &[String],
    problems: &mut Vec<String>,
) {
    for one in old {
        if !new.contains(one) {
            problems.push(format!(
                "the {member} `{one}` was removed from the {kind} `{name}` in {at}"
            ));
        }
    }
    if old != new
        && old.iter().all(|one| new.contains(one))
        && new.iter().all(|one| old.contains(one))
    {
        problems.push(format!("the {member}s of `{name}` in {at} were reordered"));
    }
}

/// One function's signature: its kind (a method, an async function, a
/// constructor), its parameters' types in order — their names are not
/// types, a name-only change is not breaking — and its result.
fn function_problem(
    at: &str,
    name: &str,
    old: &FunctionSnap,
    new: &FunctionSnap,
    problems: &mut Vec<String>,
) {
    let function = message_name(name);
    if old.kind != new.kind {
        problems.push(format!(
            "the function `{function}` in {at} changed: {} → {}",
            old.kind, new.kind
        ));
    }
    if old.params.len() != new.params.len() {
        problems.push(format!(
            "the function `{function}` in {at} changed: its parameters went from ({}) to \
             ({})",
            types_of(&old.params),
            types_of(&new.params)
        ));
    } else {
        for ((old_name, old_type), (_, new_type)) in old.params.iter().zip(&new.params) {
            if old_type != new_type {
                problems.push(format!(
                    "the function `{function}` in {at} changed: its parameter `{old_name}` \
                     changed type: {old_type} → {new_type}"
                ));
            }
        }
    }
    if old.result != new.result {
        problems.push(format!(
            "the function `{function}` in {at} changed: its result went from {} to {}",
            result_of(old.result.as_ref()),
            result_of(new.result.as_ref())
        ));
    }
}

/// A world's imports or exports, by key: a key removed or renamed is
/// breaking — it is the name of the contract — an added key is not.
fn diff_items(
    world: &str,
    what: &str,
    verb: &str,
    old: &BTreeMap<String, WorldItemSnap>,
    new: &BTreeMap<String, WorldItemSnap>,
    problems: &mut Vec<String>,
) {
    for (key, old_item) in old {
        match new.get(key) {
            None => problems.push(format!("the world `{world}` no longer {verb} `{key}`")),
            Some(new_item) => item_problem(world, what, verb, key, old_item, new_item, problems),
        }
    }
}

/// One world import or export: the interface it names (an inline one by
/// its shape), the function it holds, or the type it exports.
fn item_problem(
    world: &str,
    what: &str,
    verb: &str,
    key: &str,
    old: &WorldItemSnap,
    new: &WorldItemSnap,
    problems: &mut Vec<String>,
) {
    match (old, new) {
        (WorldItemSnap::Interface(old_path), WorldItemSnap::Interface(new_path)) => {
            if old_path != new_path {
                problems.push(format!(
                    "the {what} `{key}` of the world `{world}` changed: `{old_path}` → \
                     `{new_path}`"
                ));
            }
        }
        (WorldItemSnap::Inline(old_inline), WorldItemSnap::Inline(new_inline)) => {
            let at = format!("the `{key}` interface the world `{world}` {verb}");
            diff_types(&at, &old_inline.types, &new_inline.types, problems);
            diff_functions(&at, &old_inline.functions, &new_inline.functions, problems);
        }
        (WorldItemSnap::Function(old_function), WorldItemSnap::Function(new_function)) => {
            let at = format!("the `{key}` function the world `{world}` {verb}");
            function_problem(&at, key, old_function, new_function, problems);
        }
        (WorldItemSnap::Type(old_type), WorldItemSnap::Type(new_type)) => {
            let at = format!("the world `{world}`");
            type_problem(&at, key, old_type, new_type, problems);
        }
        // The key stayed, what it holds did not: as much a change of the
        // contract as a rename.
        _ => problems.push(format!(
            "the {what} `{key}` of the world `{world}` changed from {} to {}",
            item_of(old),
            item_of(new)
        )),
    }
}

/// How a world item is named in a message.
fn item_of(item: &WorldItemSnap) -> &'static str {
    match item {
        WorldItemSnap::Interface(_) => "an interface",
        WorldItemSnap::Inline(_) => "an inline interface",
        WorldItemSnap::Function(_) => "a function",
        WorldItemSnap::Type(_) => "a type",
    }
}

/// A function's name as messages name it: resource functions are keyed
/// by wit-parser's mangled names, `[method]r.f`, whose prefix says what
/// the name already does.
fn message_name(name: &str) -> &str {
    for prefix in ["[method]", "[static]", "[constructor]"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest;
        }
    }
    name
}

/// The types of a function's parameters, as one text: the parameter
/// list's shape, names aside.
fn types_of(params: &[(String, TypeSnap)]) -> String {
    params
        .iter()
        .map(|(_, ty)| ty.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A result's type, or its absence, as one text.
fn result_of(result: Option<&TypeSnap>) -> String {
    match result {
        Some(ty) => ty.to_string(),
        None => "no result".to_owned(),
    }
}

/// A variant case's payload, or its absence, as one text.
fn payload(of: Option<&TypeSnap>) -> String {
    match of {
        Some(ty) => ty.to_string(),
        None => "no payload".to_owned(),
    }
}

/// The parts as one comma-separated text, for messages.
fn joined<T: fmt::Display>(parts: impl Iterator<Item = T>) -> String {
    parts
        .map(|part| part.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for TypeSnap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeSnap::Primitive(name) => f.write_str(name),
            // The package's own prefix adds nothing a message needs.
            TypeSnap::Named(path) => {
                f.write_str(path.strip_prefix("pane:extension/").unwrap_or(path))
            }
            TypeSnap::List(ty) => write!(f, "list<{ty}>"),
            TypeSnap::Option(ty) => write!(f, "option<{ty}>"),
            TypeSnap::Result(ok, err) => match (ok, err) {
                (None, None) => f.write_str("result"),
                (Some(ok), None) => write!(f, "result<{ok}>"),
                (None, Some(err)) => write!(f, "result<_, {err}>"),
                (Some(ok), Some(err)) => write!(f, "result<{ok}, {err}>"),
            },
            TypeSnap::Tuple(types) => write!(f, "tuple<{}>", joined(types.iter())),
            TypeSnap::Map(key, value) => write!(f, "map<{key}, {value}>"),
            TypeSnap::Handle(own, ty) => {
                write!(f, "{}<{ty}>", if *own { "own" } else { "borrow" })
            }
            TypeSnap::Record(fields) => write!(
                f,
                "record {{ {} }}",
                joined(fields.iter().map(|(name, ty)| format!("{name}: {ty}")))
            ),
            TypeSnap::Variant(cases) => write!(
                f,
                "variant {{ {} }}",
                joined(cases.iter().map(|(name, ty)| match ty {
                    Some(ty) => format!("{name}({ty})"),
                    None => name.clone(),
                }))
            ),
            TypeSnap::Enum(cases) => write!(f, "enum {{ {} }}", joined(cases.iter())),
            TypeSnap::Flags(flags) => write!(f, "flags {{ {} }}", joined(flags.iter())),
            TypeSnap::Resource => f.write_str("resource"),
            TypeSnap::Stream(ty) => match ty {
                Some(ty) => write!(f, "stream<{ty}>"),
                None => f.write_str("stream"),
            },
            TypeSnap::Future(ty) => match ty {
                Some(ty) => write!(f, "future<{ty}>"),
                None => f.write_str("future"),
            },
            TypeSnap::FixedList(ty, length) => write!(f, "list<{ty}, {length}>"),
            TypeSnap::Unknown => f.write_str("an unknown type"),
        }
    }
}

/// The guard's failure: every breaking change, one line each, and the
/// remedy — the extension API version must move, or the change must be
/// made additive.
fn failure(problems: &[String]) -> String {
    format!(
        "wit/ changed incompatibly under a version that has already shipped:\n{}\nThe \
         extension API version in wit/ must move — a new minor before 1.0, a new major \
         from 1.0 (ADR 0046) — or the change must be made additive",
        problems.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The WIT the other tests change: one interface with a kind of
    /// every type, a second interface, a world importing a foreign and
    /// a local interface, and one exporting a local one.
    const OLD: &str = "\
package pane:extension@0.1.0;

interface greeter {
  record greeting {
    name: string,
    words: string,
  }

  record envelope {
    text: string,
  }

  variant mood {
    happy,
    grumpy(string),
  }

  variant tone {
    bright,
    dark,
  }

  variant size {
    big(u32),
    small,
  }

  enum color {
    red,
    green,
  }

  flags caps {
    loud,
    bold,
  }

  type count = u32;

  resource session {
    close: func();
  }

  greet: func(greeting: greeting) -> result<string, string>;
  hello: func(name: string, times: count) -> string;
  wave: func();
  list-moods: func(moods: list<mood>) -> list<tuple<string, u32>>;
}

interface helper {
  assist: func();
}

world greeter-world {
  import wasi:cli/stdout@0.3.0;
  import greeter;
}

world provider {
  export greeter;
}
";

    /// The same version with every breaking change the guard knows: a
    /// record reordered, a field added, a case removed, retyped and
    /// reordered, an enum and a flags reordered, a flag removed, an
    /// alias retyped, a method removed, a parameter retyped, a function
    /// removed and one made async, an interface and a world removed,
    /// and a world's imports dropped.
    const BREAKING: &str = "\
package pane:extension@0.1.0;

interface greeter {
  record greeting {
    words: string,
    name: string,
  }

  record envelope {
    text: string,
    punct: string,
  }

  variant mood {
    happy,
  }

  variant tone {
    dark,
    bright,
  }

  variant size {
    big(string),
    small,
  }

  enum color {
    green,
    red,
  }

  flags caps {
    loud,
  }

  type count = u64;

  resource session;

  greet: func(greeting: string) -> result<string, string>;
  wave: async func();
  list-moods: func(moods: list<tuple<string, u32>>) -> list<tuple<string, u32>>;
}

world greeter-world {
}
";

    /// The same version with every change that is not breaking: members
    /// and items added everywhere a member or item can be, and a
    /// parameter renamed.
    const ADDITIVE: &str = "\
package pane:extension@0.1.0;

interface greeter {
  record greeting {
    name: string,
    words: string,
  }

  record envelope {
    text: string,
  }

  variant mood {
    happy,
    grumpy(string),
    quiet,
  }

  variant tone {
    bright,
    dark,
  }

  variant size {
    big(u32),
    small,
  }

  enum color {
    red,
    green,
    blue,
  }

  flags caps {
    loud,
    bold,
    italic,
  }

  type count = u32;

  type extra = u64;

  resource session {
    close: func();
    ping: func();
  }

  greet: func(greeting: greeting) -> result<string, string>;
  hello: func(who: string, times: count) -> string;
  wave: func();
  list-moods: func(moods: list<mood>) -> list<tuple<string, u32>>;
  farewell: func();
}

interface helper {
  assist: func();
}

interface newer {
  one: func();
}

world greeter-world {
  import wasi:cli/stdout@0.3.0;
  import greeter;
  import helper;
}

world provider {
  export greeter;
}

world newer-world {
  import newer;
}
";

    /// The snapshot of one `pane:extension` package, with a `wasi:cli`
    /// package of `version` stubbed beside it so a world can import it.
    fn snapshot_with(wasi: &str, text: &str) -> Snapshot {
        let mut resolve = Resolve::new();
        let stub = format!(
            "package wasi:cli@{wasi};\n\ninterface stdout {{\n  print: func(text: string);\n}}"
        );
        resolve
            .push_str("wasi.wit", &stub)
            .expect("the stub parses");
        resolve
            .push_str("pane.wit", text)
            .expect("the package parses");
        snapshot_of(&resolve).expect("one pane:extension package")
    }

    /// The snapshot of one `pane:extension` package.
    fn snapshot(text: &str) -> Snapshot {
        snapshot_with("0.3.0", text)
    }

    /// The problems of changing the WIT `old` into `new`.
    fn problems(old: &str, new: &str) -> Vec<String> {
        breaking_changes(&snapshot(old), &snapshot(new))
    }

    #[test]
    fn the_snapshot_names_what_the_wit_names() {
        let snap = snapshot(OLD);
        assert_eq!(snap.version.as_deref(), Some("0.1.0"));
        // Interfaces by name; resource functions by their mangled keys.
        let greeter = snap.interfaces.get("greeter").expect("the interface");
        assert!(greeter.types.contains_key("count"));
        assert!(greeter.types.contains_key("session"));
        assert!(greeter.functions.contains_key("greet"));
        assert!(greeter.functions.contains_key("[method]session.close"));
        // Worlds by name; imports and exports by the qualified paths
        // that name them, this package's without their version and a
        // foreign one's with it.
        let world = snap.worlds.get("greeter-world").expect("the world");
        assert!(world.imports.contains_key("pane:extension/greeter"));
        assert!(world.imports.contains_key("wasi:cli/stdout@0.3.0"));
        assert!(
            snap.worlds["provider"]
                .exports
                .contains_key("pane:extension/greeter")
        );
        // A type's shape and a reference's snapshot.
        assert_eq!(
            greeter.types["greeting"],
            TypeSnap::Record(vec![
                ("name".to_owned(), TypeSnap::Primitive("string")),
                ("words".to_owned(), TypeSnap::Primitive("string")),
            ])
        );
        assert_eq!(greeter.types["count"], TypeSnap::Primitive("u32"));
        assert_eq!(greeter.types["session"], TypeSnap::Resource);
        assert_eq!(
            greeter.functions["hello"].params[0].1,
            TypeSnap::Primitive("string")
        );
        assert_eq!(
            greeter.functions["hello"].params[1].1,
            TypeSnap::Named("pane:extension/greeter.count".to_owned())
        );
    }

    #[test]
    fn breaking_changes_are_named() {
        let problems = problems(OLD, BREAKING);
        assert_eq!(problems.len(), 17);
        for expected in [
            "the fields of the record `greeting` in the interface `greeter` were reordered",
            "the field `punct` was added to the record `envelope` in the interface `greeter`",
            "the case `grumpy` was removed from the variant `mood` in the interface `greeter`",
            "the cases of the variant `tone` in the interface `greeter` were reordered",
            "the case `big` of the variant `size` in the interface `greeter` changed type: \
             u32 → string",
            "the cases of `color` in the interface `greeter` were reordered",
            "the flag `bold` was removed from the flags `caps` in the interface `greeter`",
            "the type `count` in the interface `greeter` changed: u32 → u64",
            "the function `session.close` was removed from the interface `greeter`",
            "the function `greet` in the interface `greeter` changed: its parameter \
             `greeting` changed type: greeter.greeting → string",
            "the function `hello` was removed from the interface `greeter`",
            "the function `wave` in the interface `greeter` changed: a function → an async \
             function",
            "the function `list-moods` in the interface `greeter` changed: its parameter \
             `moods` changed type: list<greeter.mood> → list<tuple<string, u32>>",
            "the interface `helper` was removed",
            "the world `greeter-world` no longer imports `wasi:cli/stdout@0.3.0`",
            "the world `greeter-world` no longer imports `pane:extension/greeter`",
            "the world `provider` was removed",
        ] {
            assert!(
                problems.iter().any(|problem| problem.contains(expected)),
                "no problem names {expected}: {problems:?}"
            );
        }
    }

    #[test]
    fn additive_changes_pass() {
        // Additions everywhere a thing can be added, and a parameter
        // renamed: nothing an extension built for this version would
        // notice.
        assert!(problems(OLD, ADDITIVE).is_empty());
    }

    #[test]
    fn a_version_moved_passes_whatever_changed() {
        // The same breaking changes under a version that moved: the
        // guard refuses silent breaks, not version bumps.
        let bumped = BREAKING.replace("pane:extension@0.1.0", "pane:extension@0.2.0");
        assert_eq!(
            verdict(&snapshot(OLD), &snapshot(&bumped)),
            Verdict::Moved("0.2.0".to_owned())
        );
        // Under the same version, they are refused.
        assert!(matches!(
            verdict(&snapshot(OLD), &snapshot(BREAKING)),
            Verdict::Breaking(problems) if problems.len() == 17
        ));
    }

    #[test]
    fn additive_changes_pass_the_verdict() {
        assert_eq!(
            verdict(&snapshot(OLD), &snapshot(ADDITIVE)),
            Verdict::Additive("0.1.0".to_owned())
        );
    }

    #[test]
    fn a_package_without_a_version_cannot_be_judged() {
        let old = snapshot("package pane:extension@0.1.0;\n\nworld w {\n}\n");
        let new = snapshot("package pane:extension;\n\nworld w {\n}\n");
        match verdict(&old, &new) {
            Verdict::Breaking(problems) => {
                assert_eq!(problems.len(), 1);
                assert!(problems[0].contains("declares no version"));
            }
            verdict => panic!("a versionless package cannot pass: {verdict:?}"),
        }
    }

    #[test]
    fn a_rename_is_the_removal_half() {
        // A rename removes one name and adds another: the removal half
        // is what breaks, and what the guard reports.
        let old = "package pane:extension@0.1.0;\n\ninterface i {\n  one: func();\n}\n";
        let new = "package pane:extension@0.1.0;\n\ninterface i {\n  two: func();\n}\n";
        assert_eq!(
            problems(old, new),
            vec!["the function `one` was removed from the interface `i`".to_owned()]
        );
    }

    #[test]
    fn a_foreign_import_s_identity_is_breaking() {
        // A foreign import's version is its identity: wasi:cli/stdout
        // 0.3.1 is not the interface 0.3.0 was.
        let old =
            "package pane:extension@0.1.0;\n\nworld w {\n  import wasi:cli/stdout@0.3.0;\n}\n";
        let new =
            "package pane:extension@0.1.0;\n\nworld w {\n  import wasi:cli/stdout@0.3.1;\n}\n";
        assert_eq!(
            breaking_changes(&snapshot_with("0.3.0", old), &snapshot_with("0.3.1", new)),
            vec!["the world `w` no longer imports `wasi:cli/stdout@0.3.0`".to_owned()]
        );
    }

    #[test]
    fn an_inline_interface_is_compared_by_its_shape() {
        // A world's inline interface has no name of its own: its shape
        // is compared where it sits, and changing it is breaking.
        let old = "\
package pane:extension@0.1.0;\n\nworld w {\n  import thing: interface {\n    one: func();\n  }\n}\
";
        let same = old;
        let changed = "\
package pane:extension@0.1.0;\n\nworld w {\n  import thing: interface {\n    two: func();\n  }\n}\
";
        assert!(breaking_changes(&snapshot(old), &snapshot(same)).is_empty());
        // An inline interface's own name is its world and key.
        let expected = "the function `one` was removed from the `thing` interface \
             the world `w` imports";
        assert_eq!(problems(old, changed), vec![expected.to_owned()]);
    }

    #[test]
    fn the_baseline_is_the_last_commit_that_changed_the_version() {
        // Newest first: the first commit whose declarations differ from
        // its parent's is where the current version was declared.
        let commits = vec![
            commit("head", &["0.2.0"], &["0.2.0"]),
            commit("c2", &["0.2.0"], &["0.1.0"]),
            commit("c1", &["0.1.0"], &["0.1.0"]),
            commit("intro", &["0.1.0"], &[]),
        ];
        assert_eq!(baseline_commit(&commits).as_deref(), Some("c2"));
        // The version never moved since the introduction: the
        // introduction is the baseline — today's repository.
        let commits = vec![
            commit("head", &["0.1.0"], &["0.1.0"]),
            commit("c1", &["0.1.0"], &["0.1.0"]),
            commit("intro", &["0.1.0"], &[]),
        ];
        assert_eq!(baseline_commit(&commits).as_deref(), Some("intro"));
        // A bump that missed files still moved the version: the commit
        // is the declaration point (`api_versions` names the files).
        let commits = vec![
            commit("head", &["0.1.0", "0.2.0"], &["0.1.0"]),
            commit("c1", &["0.1.0"], &["0.1.0"]),
        ];
        assert_eq!(baseline_commit(&commits).as_deref(), Some("head"));
    }

    #[test]
    fn a_walk_that_cannot_decide_takes_the_oldest() {
        // Every commit agreeing with its parent cannot happen (the
        // oldest commit introduced the tree against an empty parent),
        // but the walk stays safe: the oldest commit is the baseline,
        // the most conservative one.
        let commits = vec![
            commit("head", &["0.1.0"], &["0.1.0"]),
            commit("c1", &["0.1.0"], &["0.1.0"]),
        ];
        assert_eq!(baseline_commit(&commits).as_deref(), Some("c1"));
        assert_eq!(baseline_commit(&[]), None);
    }

    /// One commit of the walk, newest first: its sha, the versions its
    /// files declare, and the versions its parent's files declare.
    fn commit(
        sha: &str,
        versions: &[&str],
        parent: &[&str],
    ) -> (String, BTreeSet<String>, BTreeSet<String>) {
        (
            sha.to_owned(),
            versions.iter().map(|version| version.to_string()).collect(),
            parent.iter().map(|version| version.to_string()).collect(),
        )
    }

    #[test]
    fn the_version_set_reads_what_git_grep_prints() {
        // git grep -h prints each matching line, one per declaration.
        let text = "\
package pane:extension@0.1.0;\n\
package pane:extension@0.1.0;\n\
package pane:extension@0.2.0;\n\
";
        assert_eq!(
            versions_of(text),
            BTreeSet::from(["0.1.0".to_owned(), "0.2.0".to_owned()])
        );
        // Nothing matched (the tree before the introduction): the empty
        // set.
        assert!(versions_of("").is_empty());
        // An indented declaration, and another package's.
        let text = "  package pane:extension@0.3.0;\npackage pane:other@9.9.9;\n";
        assert_eq!(versions_of(text), BTreeSet::from(["0.3.0".to_owned()]));
    }

    #[test]
    fn the_last_release_tag_is_the_greatest_version() {
        // The repository has no release tags today; when it does, the
        // last release is the greatest vX.Y.Z. A tag of another shape —
        // a moving tip's, a pre-release's — is not a release.
        let tags: Vec<String> = [
            "v0.1.0",
            "v10.2.0",
            "v0.2.0",
            "release",
            "v0.1",
            "v0.1.0-rc",
        ]
        .iter()
        .map(|tag| tag.to_string())
        .collect();
        assert_eq!(last_release_tag(&tags).as_deref(), Some("v10.2.0"));
        assert_eq!(last_release_tag(&[]), None);
        let other: Vec<String> = vec!["not-a-tag".to_owned()];
        assert_eq!(last_release_tag(&other), None);
    }
}
