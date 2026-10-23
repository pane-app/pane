# Extension packages from Git repositories

A Pane extension can be shared as a public Git repository whose root holds
its `pane.json`. Pane fetches the one revision the user names and installs
it itself. The user needs no Git, compiler or other tool, and nothing in
the repository runs to install it.

The design follows [ADR 0012](adr/0012-pi-style-source-identity.md)'s Git
identity and the proposed
[ADR 0021](adr/0021-pane-fetches-git-packages-itself.md).

## Installing one

There are two ways to start:

- In root search, choose **Install extension from Git…**, the row after
  "Install extension from npm…". It opens a form of Pane's own with one
  field, the repository: its address, optionally followed by `@` and a
  branch, tag or commit. **Show package** fetches it and shows the
  preview. Back returns to root search and discards a fetch in progress.
- From the command line, `pane --install git:<address>[@<reference>]`
  opens the same preview.

Which branch, tag or commit to name, and what that means for updates, is
in [References, tracking and pins](#references-tracking-and-pins).

### Addresses

Any of these addresses works, with or without `git:` or `git+` before it:

- `https://github.com/owner/repo`
- `https://github.com/owner/repo.git`
- `github.com/owner/repo`
- `git@github.com:owner/repo.git`
- `ssh://git@github.com/owner/repo`

Pane has no SSH. It fetches each address over HTTPS from the same host and
path, as written, `.git` and case included. The preview says when an SSH
address was fetched over HTTPS.

### The preview

The preview is the one a folder has, with these lines of its own:

- "Source: Git repository github.com/owner/repo": the identity.
- The revision, and whether it is tracked or pinned:
  - "Revision: the default branch, main, tracked: an update fetches that
    branch again"
  - "Revision: branch release, tracked: …"
  - "Revision: tag v1.0.0, which you named: installing pins it to that
    revision"
  - "Revision: commit 1a2b3c4d5e6f, which you named: …"
  - for an installed pinned copy, "…, which it is pinned to: name another
    branch, tag or commit to change it".
- "Fetched: commit <full id> “<commit subject>”, served at <address>; each
  object checked against its id". For an SSH address it reads "…, SSH
  address fetched over HTTPS from https://…; …". The line says where the
  commit was served, not who made it
  ([provenance](#a-commit-id-pins-contents-not-provenance)). The subject,
  like any text the server chooses, is shown without control characters or
  format characters (bidirectional, zero-width, tags) and cut to 200
  characters.
- For a commit named by its id that no branch or tag of the repository
  points to: "Caution: no branch or tag of github.com/owner/repo points to
  commit 1a2b3c4d5e6f. A host that shares storage between forks, as GitHub
  does, can serve a fork's or a pull request's commit at this address, so
  its id alone does not show that this repository made it".
- "Pane builds nothing and runs no repository hooks, scripts or
  submodules". What the package does run, its components and any native
  helpers it ships, is listed as the preview's "Commands:", "Operations:"
  and "Helpers:".

### What Install does

Install copies `pane.json`, the components it names and this system's
helper files into Pane, as for a folder. Its commands then run like any
other. The package is recorded in `installed.json` as:

```
"git": "<host>/<path>", "gitUrl": "<address fetched>",
"gitRef": "refs/tags/v1.0.0", "gitCommit": "<id>", "pinned": true
```

- There is no `gitRef` for the default branch or a commit named by its id.
- `pinned` is there for a tag or a commit.
- For one extension of a [collection](#one-extension-of-a-collection-307),
  `"gitExtension": "<id>"` sits beside them.

The revision is written into a download folder of its own under the data
folder's `extensions/downloads/`, the one npm downloads use. The folder is
removed once the preview is shown, once an install ends and on every
failure.

## Layout: release and source-only revisions

A repository is a package when its root holds `pane.json`. A revision (a
commit) can be installed when its tree also holds the built components
`pane.json` names and this system's helper files. That is a **release
revision**, such as a release tag, or a release branch the author commits
the build to.

A revision holding only the source is **source-only**. Pane never builds
anything, so it explains the problem and installs nothing:

> "The default branch, main (commit 2cefb4e216e6) of the Git repository
> github.com/owner/repo holds only the source of "Greeter from Git": its
> built component dist/git_greeter.wasm is not in it. Pane does not build
> packages from Git or run anything in a repository; install a release
> revision whose commit includes the built components (its author's
> release tag or branch), or build it yourself and install the folder"

Not every tag of a repository is installable, and Pane does not look for
one that is.

The author example is [`guests/git/greeter`](../guests/git/greeter), a
Rust package's source. Its README gives the steps from source to release
revision
([Publishing a package from a Git repository](../guests/README.md#publishing-a-package-from-a-git-repository)).
The controlled repository the tests and smokes make from it has the source
alone on `main` and the build on the branch `release`, tagged `v0.1.0`.

## Identity

The identity is the repository without its reference: `git:`, then the
host in lowercase (with its port, unless it is HTTPS's 443), then the path
without a trailing `/` or `.git`. For example, `git:github.com/owner/repo`.

github.com, gitlab.com, bitbucket.org and codeberg.org serve a repository
at any case of its path. On those hosts the path is lowercased as well and
`.git` dropped in any case, so `github.com/Owner/Repo` and
`github.com/owner/repo` are one package (the user's decision). On any
other host, or one of those on another port, the path keeps its case,
since a server may tell the two apart.

Every address form above names the same package. A second install is
refused ("Already installed from Git repository github.com/owner/repo; use
Update to replace the installed copy"), and choosing it again, with any
reference, offers **Update**.

A copy of the same code from a folder or from npm is another package. It
is installed beside the Git one and never changed by it: Pane does not
merge sources, move data between them or infer personal and project
scopes. Moving a repository to another host or path makes another package.

## One extension of a collection (#307)

A repository whose root holds `pane-collection.json` rather than
`pane.json` is a **collection**
([ADR 0044](adr/0044-a-git-repository-holds-one-extension-or-a-collection.md)).
It lists the extensions it offers, each by an id (lowercase letters,
digits and `-`) and the folder holding it, with an optional `renamed` map
of ids it renamed or removed.

### Naming one extension

`#<id>` after the repository names one of the extensions. A reference
follows it as a reference follows a repository:
`git:github.com/owner/tools#clock@refs/tags/clock/v1.2.0`.

That extension installs as a package of its own. Its identity is
`git:github.com/owner/tools#clock`, and like any installed package it gets
its own preview (which names the extension), managed copy, record,
disabled state, uninstall and data. A second install of the same identity
is refused. Another extension of the same collection, or the repository's
own package were it one, installs beside it.

Only the files under the extension's folder reach its managed copy, so the
extension must be self-contained. A component outside the folder is
missing, and the revision is explained as source-only.

An index that cannot be taken is refused, saying what is wrong:

- a root holding both manifests
- an unknown field
- a duplicate id or path
- a malformed id
- an id reused through `renamed`
- a path that is not a plain relative folder
- a path naming no package

### Choosing from a collection

Naming a collection without an id, by address or by a picked folder, opens
the choice. It lists the collection's extensions, each with its package's
own icon, title, description and version. The user ticks some or all to
install, and each one is installed as a package of its own. A `#<id>` on a
one-extension repository is refused.

A local folder takes `#<id>` the same way (`pane --install <folder>#<id>`;
the part after the last `#` is the id), with the identity
`local:<folder>#<id>`. A dependency's `git:` or `local:` source may also
name one extension of a collection, following the conflict rules a
dependency's source already follows.

### Releases

Each extension is released on its own, its releases tagged
`<id>/v<semver>` (Go's convention for modules in subfolders), so a fix to
one never forces a version on the rest. The repository's own tags (other
extensions' and `v<semver>`) are never taken for one's.

An extension installed from its own release tag
[updates by itself](#updating-by-itself) to the newest release above the
version installed, pinned to that tag's commit. One on a tracked reference
updates as any Git package does. The newer revision is read through the
collection's index, so an extension whose folder moved within it still
updates and keeps its identity.

An id the newer index renames is followed, and the identity kept. An id it
no longer offers is reported in the update results as a notice. The
extension keeps running its installed code and is never uninstalled
silently.

## References, tracking and pins

| Named | Installs | An update without a reference |
| --- | --- | --- |
| nothing | the default branch's commit (its `HEAD`) | fetches the default branch again (tracked) |
| `@main`, `@refs/heads/main` | that branch's commit | fetches that branch again (tracked) |
| `@v1.0.0`, `@refs/tags/v1.0.0` | the commit the tag points to | installs that tag again (pinned) |
| `@<40 hexadecimal digits>` | that commit | installs that commit again (pinned) |

Naming another reference changes the recorded one. A name that is both a
branch and a tag is refused until written `refs/heads/…` or `refs/tags/…`.
An abbreviated commit id is not accepted.

An update keeps the identity, its data, whether it is disabled, its
hotkeys and aliases, as a folder's does. A tracked branch also updates
[by itself](#updating-by-itself). A Git package has no **Reload** or
**Develop** row, since it has no source folder on this computer.

### A commit id pins contents, not provenance

A commit id pins the bytes: the files are exactly that commit's tree. It
does not show that the repository at the address made the commit.

GitHub, and other hosts that share object storage between a repository
and its forks, serve a commit that exists only in a fork, or in a pull
request never merged, at the upstream repository's address too. So
`trusted/repo@<id>` can install code the owners of `trusted/repo` never
accepted.

Pane cannot tell from the server where a commit came from. When a commit
is named by its id, Pane also lists the repository's `HEAD`, branches and
tags (peeled), and cautions on the preview when none of them points to
that commit. It does this for the package previewed, and for each Git
dependency installed with a package ("Caution (Greeter from Git): no
branch or tag of … points to commit …", below its "Requires:" line).

A branch or a tag is resolved from the repository's own listing, so a
fork's commit is never installed through one. When choosing what to pin,
prefer a tag the repository's owners published.

## Updating by itself

An eligible Git package updates by itself, as
[an npm package does](npm.md#updating-by-itself) (#50).
[Which packages update](#which-packages-update) is below.

It uses the same updater thread as npm packages, on the same cadence: a
minute after Pane starts, then every 24 hours, by the launcher's clock.
The same controls apply: the global row in the extension list and a
per-package one, in `updates.json`.

An update applies at the same safe activation boundary. It never applies
during a command of the package that the user asked for and that has not
answered, nor while one of the package's screens is on display. Pane
retries every second until the package is quiet, and the update results
list it as "Waiting until <title> is not in use" until it applies.

It applies as the preview's Update row does. The identity, saved data,
disabled state, hotkeys and aliases are kept. The old generation ends and
managed background work restarts with the new code; a component that opts
in to the [state handoff](generations.md#the-state-handoff) (ADR 0041,
#159) keeps what it had in memory. The outcome is the pass's update
results, where it is a quiet row with the old and the new commit
("1a2b3c4d5e6f → 6f5e4d3c2b1a").

The user can also ask for a check at once, with root search's **Check for
Extension Updates** command or the Settings Extensions page's **Check for
updates** button. That check looks at every tracked package, turned off,
disabled and paused ones included, and its toast follows the pass (see
[npm](npm.md#updating-by-itself)).

### What a check reads and fetches

This is where Git differs from npm. For a tracked package, a check asks
the repository for its reference listing alone: the same `ls-refs` a
preview's fetch begins with, without fetching a commit. Nothing is fetched
while the tracked branch points at the commit installed.

When the branch has moved, the new commit is fetched (with `deepen 1`, as
an install fetches it) and checked exactly as an install checks a package.

A check or an update can fail: the repository is gone, the branch deleted,
the moved-to revision source-only, or its manifest, API, components or
helpers no longer pass. The failure is a Failed row in the update results,
announced once the next time the launcher is shown, and the installed copy
is left as it is. For example:

> "It was not updated: Branch release (commit …) of the Git repository …
> holds only the source of …; It keeps running its installed code"

### Extensions of a collection

One extension of a collection installed from its own release tag
`<id>/v<semver>` (ADR 0044) is checked differently. The repository's tags
with the extension's prefix (`ls-refs` with `refs/tags/<id>/`) alone say
what its newest release is. Nothing is fetched while the newest names the
version installed, and the repository's other tags, other extensions' and
`v<semver>`, are never taken for its.

When a newer release exists, the tag's commit is fetched and checked as an
install checks a package. It is read through the collection's index, so an
extension whose folder moved within it still updates, and the extension is
pinned to that tag's commit. The update is its own and never moves another
extension of the collection.

Where the index no longer lists the id as an extension, the `renamed` map
decides (ADR 0044, #310):

- An entry naming the id's new one is followed. The newer revision's files
  are read from the new id's folder, and the installed copy's identity is
  kept, as a moved folder's is.
- An id that is gone, or mapped to `null`, is reported in the update
  results as a notice ("The collection no longer offers this extension. It
  keeps running its installed code."). The row has no Retry. The installed
  copy keeps running and is never uninstalled silently.

Check for Update on the extension's page previews that one extension by
its repository and id, at its newest release above the version installed.
This is the ordinary preview, whose Update keeps or changes the recorded
reference as a Git package's does. A renamed id resolves through the map,
so the preview shows the extension under its current id.

### Which packages update

As with [npm's](npm.md#which-packages-update), a Git package updates by
itself when it is enabled, not paused after a failure, not turned off, and
one of these:

- an installed Git package that is tracked: installed from the default
  branch or a named branch
- one extension of a collection, installed from its own release tag

Any other tag, or a commit named to install it, pins the package. An
update installs that revision again, so it is never updated by itself.

The same safe boundary defers an update while a command runs. An update
the user chose (choosing the repository again, Update on its preview,
`pane --install git:<address>`) replaces the copy without waiting, as a
reload does.

Two choices are provisional, pending the user's decision: the shared
cadence and retry timing (see [npm](npm.md#updating-by-itself)), and that
a tracked branch is followed even when its new commit holds an older
version, as `latest` is.

## Dependencies from Git

A `pane.json` dependency's `source` may be `git:<address>` or
`git:<address>@<reference>` ([dependencies](dependencies.md#declaring)),
in a local, npm or Git package. The plan, claims and rollback are #42's.

- A missing required one is fetched while the preview is worked out,
  listed as "Requires: Greeter from Git, installed with it from
  git:https://github.com/owner/repo@v1.0.0", and installed first, at the
  reference named (tracked or pinned as above). A commit named by its id
  that no branch or tag points to gets a caution on the next line, as for
  a package ([provenance](#a-commit-id-pins-contents-not-provenance)).
- An installed one is used as it is and never fetched again, unless the
  source names a reference the installed copy is not at: "Nothing was
  installed: Caller requires Greeter from Git at v1.0.0, and branch release
  (commit …) is installed; Pane does not replace the installed copy while
  installing another extension: update it to v1.0.0 (Git repository
  github.com/owner/repo@v1.0.0) if Caller needs that revision". Two
  dependents naming different references conflict likewise.
- Its record is `{ "id": "greeter", "git": "<host>/<path>" }`. A call by
  the dependency id reaches it, and a call by identity takes `git:` and
  any address form of the repository, without a reference.
- A package from npm or Git cannot name a `local:` folder: "… comes from
  Git but names the local folder `local:../helper` as its dependency
  `helper`; a package published to npm or Git can depend only on packages
  from npm or Git".

## What is refused

Each refusal is explained on the preview, which then offers nothing.
Nothing is installed or left in the downloads folder.

- Not an address Pane fetches: "`ftp://…` is not a Git repository address:
  Pane fetches Git repositories over HTTPS, and does not use `ftp://`".
  Also plain `http://` ("… only over HTTPS …"), credentials in the
  address, `?`, `%` and `..`. A `#` after the repository path names one
  extension of a [collection](#one-extension-of-a-collection-307), so it
  is taken as the id rather than refused.
- No repository there: "There is no Git repository at https://…"
- A private repository: "The Git repository … asks to sign in (its server
  answered 401): Pane sends no credentials, so it installs only from
  public repositories"
- A redirect: "…/info/refs?service=git-upload-pack answered 301, sending
  Pane elsewhere: Pane follows no redirect, so name the repository by the
  address it moved to"
- An older server: "The Git repository … is not served with Git's
  protocol version 2 over HTTPS, which Pane needs …". SHA-256
  repositories are refused too.
- No such reference: "The Git repository … has no branch or tag named
  nope"; "… has both a branch and a tag named x; name the one to install
  as …@refs/heads/x or …@refs/tags/x"; and for a commit it does not have,
  "Could not fetch commit … of …: …"
- Too large:
  - a reference listing over 16 MiB, for a branch or tag (for a commit
    named by its id, such a listing is taken as naming no branch or tag
    that points to it, so the preview cautions rather than refuses)
  - a pack over 64 MiB
  - more than 20,000 objects
  - more than 256 MiB of its contents in memory at once (entries inflated
    and objects its deltas make, together)
  - a chain of more than 4096 deltas
  - files over 256 MiB
  - more than 10,000 files and folders
  - folders more than 32 deep
- A damaged or forged pack: its checksum, a zlib stream, a delta, an
  object whose id does not match (so the tree is not the commit's), or
  data crafted to collide under SHA-1.
- An unsafe tree: "… cannot be installed safely: its tree contains
  `dist/link`, a symbolic link; Pane takes only files and folders every
  system can write". Also refused:
  - a submodule
  - a `.git` or `git~1` entry, in any case, and with the zero-width and
    direction characters HFS+ ignores in a name, as Git checks
  - two names differing only in case
  - an entry that is not one plain name (`a/b`, `../x`, an absolute
    `/path`, `\`, `.`, `..`), refused before anything is written outside
    the download folder
  - a name longer than 255 bytes, refused before the rest of it is read,
    and shown from its start only
  - every name npm's unpacking refuses ([npm](npm.md#what-is-refused))
- Not a Pane extension: "… is not a Pane extension: it has no pane.json at
  the repository's root …"
- Source-only: see [above](#layout-release-and-source-only-revisions).
- Git LFS: "… stores its component dist/x.wasm with Git LFS, which Pane
  does not fetch: …"

After that, every check a folder gets applies: its manifest, WASI 0.3
imports, the extension API shape, its platforms and its helpers for this
system.

## What never runs

Pane has no `git` program, library or configuration of its own. The
repository's hooks, `.gitattributes` filters and line-ending conversions,
`core.fsmonitor`, `core.sshCommand`, `uploadpack` and `protocol.allow`
settings and `GIT_*` variables cannot apply, and files are written exactly
as committed. Submodules and Git LFS objects are never fetched. Build
scripts (`build.rs`, `package.json` scripts, Makefiles) are never run.

## The client

The requests go through Pane's one HTTP client: hyper, rustls and the
system's certificates
([ADR 0018](adr/0018-extensions-reach-the-network-through-wasi-http.md),
[ADR 0019](adr/0019-pane-downloads-npm-packages-itself.md)).

- a new connection per request
- no proxy, no redirect, no credentials
- a User-Agent of `git/pane-<version>`
- at most 30 s to connect, 60 s between pieces and 5 minutes per request

The fetch asks for the one commit without history (`deepen 1`) when the
server offers it.

Only a development build or a test fetches plain `http://`, and only from
a loopback address written as one. The tests and smokes serve their
repositories that way
([`repo_server.rs`](../crates/pane-core/tests/support/repo_server.rs),
[`scripts/repository_server.py`](../scripts/repository_server.py)), so
none of them reaches the network. Those servers, not Pane, run
`git upload-pack`, with none of the user's Git configuration.

### Fetching a collection in parts

The fetch is made in parts where the server advertises the partial-clone
filter (`uploadpack.allowFilter`, as GitHub's and GitLab's servers do) and
the revision holds a collection
([#311](https://github.com/pane-app/pane/issues/311),
[ADR 0044](adr/0044-a-git-repository-holds-one-extension-or-a-collection.md)):

1. The commit and its trees are fetched with `filter blob:none`, without
   any file contents.
2. Blobs are then fetched by naming their ids (`want <blob id>`, the same
   lazy fetch Git's own partial clones make of missing objects). First
   comes the collection's listing: the index, each extension's manifest,
   and the icon a manifest names, so the choice shows each extension's own
   details.
3. As the user chooses, one fetch takes every extension chosen from the
   same revision together. The revision is never fetched twice, and an
   install by its id fetches its one extension's folder whole at once.

The trees are walked in memory to find the chosen extensions' folders.
Every object in every pack is checked against its id as any pack's is, and
a wanted blob the server does not send is refused, not skipped.

The limits still bound what is taken. Each pack is bounded on its own
(64 MiB, 20,000 objects). The files and folders written, the listing's and
the chosen extensions' together, share the one budget of files and folders
and their size.

A server that advertises no filter, and a repository that is one extension
rather than a collection, are fetched whole as above. Looking for the
index first costs such a repository one small extra fetch of its trees.

### Trying a real host by hand

No check reaches a real Git host. A contributor can try one by hand, with
a public repository holding a Pane package's release revision:

1. `cargo build --release -p pane`.
2. `PANE_DATA_DIR=<a new folder> target/release/pane --install
   git:https://github.com/<owner>/<repo>@<tag>`. The preview's "Fetched:"
   line names the commit; compare it with `git ls-remote <address> <tag>`
   (its peeled `^{}` line for an annotated tag).
3. Choose **Install**, run its command, and check that
   `<folder>/extensions/installed.json` records `gitRef` and `gitCommit`,
   and that `<folder>/extensions/downloads/` is empty.
4. To see a refusal, name the repository's development branch if it holds
   only the source, or any repository without `pane.json`.

No one has tried this against GitHub, GitLab or another host yet.

## Checks

- Unit tests in [`git.rs`](../crates/pane-core/src/git.rs):
  - address forms and their identity, and references
  - HTTPS only (plain HTTP from a loopback address in tests)
  - case folded on github.com, gitlab.com, bitbucket.org and codeberg.org
    only
  - SSH addresses noted
  - pkt-lines
  - packs: checksum, deltas by offset and by id, a missing base, object
    counts, 2,000 deltas against ids written base last, a chain of 4096
    deltas and one longer, the shared memory budget with deltas'
    instructions let go
  - trees: links, submodules, `.git`, unsafe names, names differing in
    case, limits of entries, size and depth
  - files without execute permission
  - Git LFS pointers
  - a server's text (subject, `ERR`, a connection failure) shown without
    control or format characters and cut
- Unit tests in `downloads.rs`: a name is one plain name (`a/b`, `../x`,
  `/abs`, `sub/.git`, `..`, `.`, `\`).
- [`crates/pane-core/tests/repositories.rs`](../crates/pane-core/tests/repositories.rs),
  against Git's own server (`git upload-pack`) on 127.0.0.1:
  - preview, install and run from a tag, and after a restart without
    fetching again
  - the source-only default branch
  - a tracked branch moving and updated; a commit and a tag pinned
  - the identity across address forms, and a local copy of the same code
    beside it
  - a local package requiring a Git one at a tag; a dependency naming
    another revision; a Git package naming a local folder
  - a link and a submodule
  - hand-made hostile trees (`../x`, `../../x`, an absolute path, a folder
    `../x`, `dist/x`, `..\x`, `sub/.git`) writing nothing outside the
    download and leaving no download behind
  - a commit only a pull request's reference holds, previewed with the
    caution, and commits, branches and tags a reference points to without
    it
  - another case of the path on a host treated as github.com is the
    installed package, and a dependency spelled so resolves to it without
    fetching
  - hooks, filters and install scripts never running, and components taken
    as committed
  - missing repositories, references and commits, ambiguous names,
    redirects, sign-in, protocol version 0, a `# service` line, an
    unreachable server
  - the form
  - since #307: one extension of a collection installed by its id (with
    the id recorded beside the Git fields, only its folder's files in the
    managed copy, a duplicate install refused and choosing it again
    offering Update); every way an index is refused; a component outside
    the extension's folder explained as source-only; the local-folder
    form; a dependency's `git:` and `local:` sources naming one extension
    of a collection
  - since #311: a server that allows the partial-clone filter serves the
    choice and the chosen extensions' files only, the fetches it was sent
    checked against the repository's object ids; a server that allows none
    is fetched whole; an install by id from a filtering server takes only
    that extension's folder; a one-extension repository from one is
    fetched whole as ever; a collection's refusals say the same from
    either; a component stored with Git LFS is refused as one
- [`crates/pane/tests/repositories.rs`](../crates/pane/tests/repositories.rs):
  - the form and the source-only explanation
  - preview, Install in view below whole Git lines, and the command
    running in the native window at Pane's size
  - one extension of a collection previewed by its id, installed and run,
    with its page in Settings
- [`crates/pane-core/tests/update.rs`](../crates/pane-core/tests/update.rs),
  for the [automatic updates](#updating-by-itself) of Git packages, from
  the same server:
  - a tracked branch that has moved updating by itself (identity, tracked
    reference and pin kept, the new copy running)
  - a pinned tag never asked about
  - the per-package control row
  - a branch that has moved to a source-only revision refused, with the
    installed copy untouched
  - since #309, one extension of a collection installed from its own
    release tag: updating to the newest one above the version installed,
    choosing it among other extensions' tags, a prerelease and the
    repository's root `v<semver>` tag; a tracked branch moving the
    extension's folder within the collection, its identity kept; each
    extension updating on its own, one's release never moving another; a
    release whose index no longer lists the id refused; one that needs a
    newer Pane refused; a pinned tag that is not the extension's own never
    asked about; and Check for Update on its page previewing it at its
    newest release (or its installed one, when none is newer)
- The native smokes' own phase (frames 300 to 307;
  [Linux](platforms/linux.md#git-packages-46)).

## Limits

- Public repositories over HTTPS only: no SSH, credentials, proxies or
  private hosts' sign-in. SHA-256 repositories are refused.
- The package must be at the repository's root. Submodules and Git LFS
  are not fetched.
- There is no check for a newer commit other than the updater's
  [tracked-branch and release-tag checks](#updating-by-itself) and
  choosing the repository again. There is no history browser and no
  publishing to a host.
- Signed tags and commits are not checked. The commit id, checked object
  by object, is the pin, and it proves contents, not provenance. A fork's
  commit served at the repository's address is only cautioned about, on
  the preview of the package named by that commit or of the package that
  requires it. An installed dependency is not fetched again, so it is not
  checked again.
- Tags and branches are resolved on the server. A server that does not
  allow fetching an unadvertised commit id refuses a commit named by its
  id (GitHub, GitLab and Git's own server allow it).
- The pack and its objects are held in memory while the revision is
  written (at most 64 MiB and 256 MiB). A collection fetched in parts
  holds its trees while the choice is open; each of its packs is bounded
  on its own, with the files written under one budget across them
  ([The client](#the-client)).
- Only github.com, gitlab.com, bitbucket.org and codeberg.org fold the
  path's case. Another host that ignores case (a self-hosted GitLab, say)
  makes `Owner/Repo` and `owner/repo` two packages.
- The HTTPS path to a real host is not exercised by the checks, which
  never reach the network ([by hand](#trying-a-real-host-by-hand)).

## Tracking

Added for [#46](https://github.com/pane-app/pane/issues/46) (US16–US18,
US20–US24, T11, T12, G4, G6; contributions).
