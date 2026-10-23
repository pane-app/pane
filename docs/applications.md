# Applications

Pane opens installed applications from root search. Type an
application's name and it is listed, ranked with commands by title;
Enter (or a click) opens it.

The feature is a default extension, Applications, which the user can
disable like any package. Windows, macOS and Linux share one design and
differ only in where Pane looks for applications and how it opens them.
The issues and stories behind it are under [Tracking](#tracking).

## Where it lives

The spec and [ADR 0001](adr/0001-small-core.md) make app launching a
default extension that can be disabled, not core.
[ADR 0006](adr/0006-raycast-style-search-with-extension-providers.md)
keeps matching, ranking and dispatch in the core.

A pure WASI 0.3 guest can neither read the system's application folders
(its WASI context has no preopened folders) nor start a program. So the
work is split as
[ADR 0015](adr/0015-host-finds-and-opens-applications-for-an-extension.md)
records.

### The host capability, in the core

`pane:extension/applications`
([`wit/applications.wit`](../wit/applications.wit)) is a host import any
command may use. `installed()` lists the applications and `open(id)`
opens one.

Each system has an adapter behind one small trait, `Discovery`
([`pane_core::applications`](../crates/pane-core/src/applications.rs)).
The adapter reports the shortcuts, bundles or desktop entries it finds.
The host turns them into applications with a stable
[identity](#identity) and keeps that list, with the map from each id to
what opens it. The list stays [current by itself](#live-list) while a
package that asked for it runs, and the host keeps an [icon](#icons)
cache beside it.

ADR 0038 ("The host keeps a live application list and an icon cache")
amends ADR 0015's stateless adapter for this. Nothing is looked for
until a guest asks.

### The default extension

The extension's source is the
[Applications repository](https://github.com/pane-app/applications)
(Rust). A Pane release pins its release commits in
[`crates/pane/defaults.json`](../crates/pane/defaults.json).

Its command, "Applications", is a
[root provider](root-search.md#root-providers) (#164). It has no row of
its own. It supplies the applications to root search as
[indexed results](root-search.md#results-supplied-ahead-of-the-query),
each found by its name and opened by Enter. The
[applications samples](../guests/packages) exercise the same host import
and indexed results from this repository's tree.

First setup acquires the extension from the commit this Pane release pins
([#278](https://github.com/pane-app/pane/issues/278),
[#53](https://github.com/pane-app/pane/issues/53)). A user can also
install it by hand from its repository:

```
pane --install git:https://github.com/pane-app/applications
```

### Not a native helper

There is no native helper
([ADR 0014](adr/0014-optional-native-extension-helpers.md), #15).
Listing folders and asking the system to open a file are what the host
process already does on every system. A separately packaged per-OS helper
binary would add distribution and lifecycle work (#15 is not built) for
no capability the host lacks.

## Behavior

### When Pane looks for applications

Nothing runs at start, at install or for a blank query. The first query
that is not blank starts the extension, which asks the host for the
installed applications. They are kept, and every later query ranks them
at once, so typing never waits for them. The first query lists the
commands at once and the applications as soon as they are found.

The extension asks again on the first query after each return to root
search (at start, after Escape from a command, after an install), and
whenever the host's [live list](#live-list) changes. It keeps the earlier
list until the new one arrives.

The host keeps its own list
([`Cached`](../crates/pane-core/src/applications/cached.rs)). Only the
very first request scans the system's folders, on the runtime thread.
Later ones get the kept list at once, and watchers keep it current. So an
application installed while Pane is open appears within about a second,
even while root search is on screen.

A blank query lists no application, so root search's empty list stays the
commands and Pane's own rows.

### What a row shows

Each application is a root result titled with its name as the system
shows it ([names](#names)). Its subtitle is "Application", or, when
another application has its name, what tells it apart. It is drawn with
its own [icon](#icons).

It is matched and ranked like a command's title
([root search](root-search.md#matching-and-ranking)):

- "firefox" finds Firefox.
- An exact or prefix title beats a word inside another title.
- On the same rank, commands come first.

Its alternate titles (its untranslated name, its program's name: `code`,
`wt`) find it as its title does, and its keywords find it as its subtitle
does. The row shows its title whichever matched.

### Opening

Enter opens the application, off the window's thread. Once it opened,
the launcher closes, as Raycast's does, and root search shows next time
with its query empty (#468). A failure keeps the launcher open and stays
visible as the status error:

- "Could not open Firefox: cannot find the program firefox"
- "... not allowed to run ..."
- the system's own message from `open` or `ShellExecuteEx`
- "... it is no longer installed" (an id no application has) or "... no
  longer exists" (an old path), for an application removed since it was
  listed

If the applications cannot be listed at all, a row "Applications — Could
not list: ..." is listed for every query that is not blank, and Enter
shows the whole error. A missing or unreadable location only adds
nothing.

### Disabling

When Applications is disabled (Settings › Extensions):

- its applications leave root search at once;
- the kept list is dropped and every watcher stops;
- its instance is stopped, and nothing looks for applications any more;
- a list still on its way is discarded.

Other results (commands, the calculator) are untouched. Enabled again,
the next query looks, and watches, again.

## Live list

The host's list stays current by itself. ADR 0038 records why this
replaced the earlier 10-second rescan. The rules are in
[`cached`](../crates/pane-core/src/applications/cached.rs) and
[`watching`](../crates/pane-core/src/applications/watching.rs).

### Built on demand

The list is built, and its watchers start, when a running package first
asks for the installed applications (`installed()`). For the
Applications extension, that is the first query that is not blank.

Each component that asked is noted on its package's generation. Once no
package that asked can run any more (disabled, uninstalled, paused, its
code replaced), the list is dropped and every watcher stops
(`Applications::release`), until a package asks again.

Opening an application by id while no list is kept scans once, without
keeping or watching anything.

### Watchers

Pane watches with the native file watcher development mode uses
(`notify`):

- Windows: ReadDirectoryChangesW on each shortcut folder, the Start menus
  with their subfolders and the Desktops and taskbar pins flat. Any file
  changing counts, `.lnk`, `.url` and `.appref-ms` alike. For packaged
  apps it watches `%LOCALAPPDATA%\Packages`, where Windows makes a folder
  for each package registered for the user. Only a package's folder
  appearing or going counts there. Since Windows completes the
  registration afterwards, the list is rescanned again five seconds
  later.
- macOS and Linux: FSEvents on the Applications folders on macOS, and
  inotify on the `applications` data folders on Linux, with their
  subfolders.

A folder that does not exist yet is watched for from the nearest folder
above it, where only changes on the way to it count. The watch is made
again after each rescan until the folder is watched itself.

### Debounce

Reported changes are rescanned once they settle: half a second after the
last one, at most two seconds after the first.

### Grace

A rescan replaces the list by identity. An application whose last source
went stays listed (and opens) for five seconds, by the launcher's clock.
It leaves only if nothing with its identity came back meanwhile, so an
update that removes and reinstalls it never makes it flicker out.

A shortcut that now opens another program is that program at once, not a
leaving application. A renamed shortcut keeps its application, its id
and its pins, with the new title.

### Reconciling

These trigger a full rescan at once:

- a watcher that lost changes (its buffer overflowed) or failed;
- a wait that lasted far longer than it could (the system slept, or its
  clock jumped);
- every 30 minutes while the list is kept.

A failed rescan keeps the list.

### Telling the launcher

When the listed applications change, every enabled command with indexed
results that asked for the installed applications has its results marked
stale. That covers the Applications extension, the JavaScript and
TypeScript samples, and any extension using the import.

While root search is on screen with a query, those results are asked for
again at once and listed in place. A command already being asked is
waited for first, since its answer may predate the change. Otherwise they
are asked for at the next query.

The selected row stays on the same result wherever it moved, or at the
same position if that result left, so the list never jumps under the
user.

A return to root search asks such a command for nothing, since nothing
changed (#202). A command whose results nothing tells of is asked again
after each return.

No guest export is added: the guest's `results()` reads the host's
current list, which answers at once.

## Identity

Pane identifies an application by what it is, wherever its shortcut
lies, so its pins (and, once root search learns from choices, ADR 0030,
what it learned) survive its updates. The rules are pure functions in
[`identity`](../crates/pane-core/src/applications/identity.rs), tested on
every system.

### Sources and their keys

Each system's adapter reports sources (a shortcut, a packaged app, a
bundle, a desktop entry), each with the key of what it opens:

- A Windows desktop program is keyed by the shortcut's target, in
  lowercase with its arguments. The target is read through the shell's
  shortcut interface (`IShellLinkW`), on single-threaded COM apartments,
  up to eight threads at once, and each shortcut is read again only when
  its file changed.

  Every path segment that is a version becomes a wildcard keeping its
  prefix (`app-*`). A version is digits separated by one to three dots,
  optionally after a prefix of up to eight letters and one separator:
  `app-1.0.9003`, `1.2.3.4`, `v2.0`. So Discord or Slack moving into a
  new version folder stays the same application. Different arguments (two
  browser profiles, two web apps) are different applications.

  An MSI-advertised shortcut (Office's, for instance) names the product it
  installs rather than a file. It is resolved through the Windows
  Installer (`MsiGetShortcutTargetW`, `MsiGetComponentPathW`) to the
  program that product installed, so it is one application with any plain
  shortcut to that program. One whose product is not installed is left
  out.
- A Windows internet shortcut (`.url`) or ClickOnce application reference
  (`.appref-ms`) is keyed by the URL, or the deployment it names, in
  lowercase. So a game's `steam://rungameid/...` link on the Desktop and
  in the Start menu is one application.
- A Windows packaged app is keyed by its package family name. A package
  with several apps adds the AppUserModelID for every app after the first
  (by app id). A shortcut whose AppUserModelID is a packaged app's is that
  app.
- On macOS the key is the bundle identifier (`CFBundleIdentifier` of the
  bundle's `Info.plist`, XML or binary), so moving or renaming the bundle
  keeps it.
- On Linux the key is the desktop file id, as before.
- A source with nothing better (a shortcut the shell cannot read, a bundle
  without an identifier) is keyed by its own path.

### One application per key

Sources with one key are one application: two shortcuts to one program
are one result.

Its primary source is the one whose name is its title and which Pane
opens. That is the source in the most preferred place, then the one with
the shorter path:

- On Windows: the user's Desktop, every user's Desktop, the user's Start
  menu, every user's, the taskbar pins, then the Apps folder, as Explorer
  prefers a shortcut of the user's own.
- On macOS: `/Applications`, `/System/Applications`, `~/Applications`.

### Ids

An application's id is the first 128 bits of a SHA-256 digest of the
typed key, as 32 hexadecimal digits. It is the same on every start and
every machine with the same installation. Extensions receive it through
the applications import in Rust, JavaScript and TypeScript and must not
parse it.

The host keeps the map from each id, and from each source's path, to the
application. So `open(id)` and opening a target with an application
(`system.open`, Open With…, an indexed result's `open`) work for any id.

### Ids from before identities

Ids from before identities were the source's path (the shortcut, the
bundle, the desktop entry, `shell:AppsFolder\<AppUserModelID>`). `open`
still accepts them, opening the application that source belongs to.

A quick slot holding one is carried over when the command's results
arrive. It resolves to the result now listed for that application, and
`quick-slots.json` is rewritten with the new id. One whose source is gone
keeps its slot and says that Applications no longer lists it.

## Names

An application is titled as the system shows it in the user's language,
and found by the other names people know it by. The rules are pure
functions in [`names`](../crates/pane-core/src/applications/names.rs),
tested on every system.

### Title

The title is the primary source's name:

- On Windows, the name Explorer shows for the shortcut, falling back to
  the file name. That is the shell's display name, which the folder's
  `desktop.ini` `LocalizedFileNames` or the shortcut's name resource
  translate: "Paint" is "Ứng dụng Vẽ" in Vietnamese. A packaged app's
  display name is used as before.
- On macOS, the bundle's display name as Finder shows it
  (`NSFileManager`), else its folder name.
- On Linux, the `Name` for the user's messages locale (`$LC_ALL`,
  `$LC_MESSAGES`, `$LANG`), chosen as the Desktop Entry specification
  says: `lang_COUNTRY@MODIFIER`, `lang_COUNTRY`, `lang@MODIFIER`, `lang`,
  then the plain `Name`.

### Alternate titles

Alternate titles are matched as the title is. They are every other name a
source of the application has (the untranslated name: the shortcut's file
name, the bundle's folder name, the plain `Name`; another shortcut's
name), then the program's name. The program's name is the target's file
name without `.exe` (or `.bat`, `.cmd`, `.com`) on Windows, and the
program `Exec` runs on Linux. macOS adds no program name.

The program's name is left out when:

- the shortcut passes the program arguments that say what it opens. On
  Windows that is any argument. On Linux it is one that does not start
  with `-` or carries a value, so `--new-window` does not count but
  `--app-id=…` does: a browser's web app is not found by "chrome".
- it is a generic name: app, application, bootstrap, bootstrapper, client,
  config, console, env, game, helper, host, install, installer, launch,
  launcher, loader, main, program, run, server, service, settings, setup,
  shell, start, stub, tool, uninstall, uninstaller, update, updater,
  wrapper, also with an architecture suffix such as `launcher64` or
  `setup_x64`.
- another application's program has the same name, so it never picks one
  of them arbitrarily.

Each name appears once, ignoring case, and never the title itself.

### Keywords

Keywords are matched as the subtitle is. On Linux they are the entry's
`Keywords` for the user's locale (else the plain ones).

### Distinction

Applications are grouped by title, ignoring case. In a group of two or
more, each gets the first of these that no other in the group has:

1. its program's name;
2. the name of its program's folder (its source's folder when the program
   is a bare command or there is none);
3. the program's full path;
4. the source's own path.

The Applications extension shows the distinction as the subtitle instead
of "Application". A pin holding such an application shows it as its
tooltip and accessible description. An application alone with its title
has none.

The host gives each application record these names (`name`,
`alternate-titles`, `keywords`, `distinction`), so every extension using
the import gets them. An indexed result takes `alternate-titles` and
`keywords` from any extension
([root search](root-search.md#matching-and-ranking)).

## Icons

Every application row in root search, and a quick slot pinning one, shows
the application's own icon. It is drawn bare, without the tile Pane's own
rows keep (decision 2,
[ADR 0035](https://github.com/pane-app/pane/blob/2a4f9c43c990656325297a5980f34fa4bddba76e/docs/adr/0035-the-launcher-borrows-raycasts-polish-within-the-accepted-ui.md)).
The host extracts and keeps the icons (ADR 0038,
[`icons`](../crates/pane-core/src/applications/icons.rs)).

### Extraction

Icons are extracted at 256 pixels so they stay sharp at any scale, with
one adapter per system (`IconExtractor`, `NativeExtractor`).

Windows desktop programs start from the shell's image of the
application's primary source (`IShellItemImageFactory`, icon only). Above
48 pixels, an image whose content ([below](#filling-its-place)) spans
less than half its width and less than half its height is rejected. Such
an image is a small icon the shell padded into the jumbo size, or framed
in a thumbnail: a program that ships only a 32-pixel icon is drawn by the
shell as that icon in the middle of a square with a thin frame. Pane then
tries the next source, in this order:

1. the shortcut's own icon location (`IShellLinkW`, extracted at 256
   pixels, which the system scales from its closest size);
2. the shortcut's target program's shell image;
3. that program's own first icon (or a program source's), extracted the
   same way so the system takes the largest image it has;
4. the shell's file information icon (32 pixels).

Only when every source fails is a padded or framed image kept (then
cropped, below).

An internet shortcut's (`.url`) padded image is rejected for the icon its
`IconFile` and `IconIndex` name.

A ClickOnce reference (`.appref-ms`) is drawn first by its deployed
program's own icon, then that program's shell image, before the shell's
image of the reference. The program is found where ClickOnce installs it,
`%LOCALAPPDATA%\Apps\2.0\<random>\<random>\<name>_<token>_<version>_<hash>\`
([`click_once`](../crates/pane-core/src/applications/icons/click_once.rs),
tested against a store the test builds), by:

- the deployment's name, lowercased and, past ten characters, shortened
  as ClickOnce does (`orde..tion` for `Orders.application`);
- its publisher's `PublicKeyToken`;
- the newest version;
- the program named after the deployment, else its first.

Windows packaged apps are drawn by the logo the package's manifest names
for the app: `Square44x44Logo`, else `Square150x150Logo`, else the
package's `Logo`. Pane finds it in the package's install folder through
`GetPackagesByPackageFamily` and `GetPackagePathByFullName`. Of its
variants it takes the target size closest to 256 pixels (the smallest at
least that large, else the largest), unplated, for the dark theme, and
the `lightunplated` one for the light theme when the package ships it.
Failing those, it takes a plated target size, else the largest scale,
else the file as named
([`appx`](../crates/pane-core/src/applications/icons/appx.rs)). A
packaged app also found by a shortcut still draws the package's logo. A
package Pane cannot read is drawn as the shell draws it.

On macOS the icon is the workspace's icon of the bundle (`NSWorkspace`),
its largest image up to 512 pixels, as Finder and the Dock show it.

On Linux it is the desktop entry's `Icon`: a file when it is an absolute
path, else looked up by the freedesktop Icon Theme specification. The
lookup goes through the user's current theme (GTK's
`gtk-icon-theme-name`, else KDE's `[Icons] Theme`), the themes it
inherits, then `hicolor`, in `~/.icons` and the `icons` of the data
folders, then the legacy `pixmaps` folders. Within a theme it takes an
SVG of a scalable folder, or the PNG closest to 256 pixels
([`theme`](../crates/pane-core/src/applications/icons/theme.rs)).

### Filling its place

Whatever extracted the icon, and on every system (`fill_its_place`), an
image whose content spans less than three quarters of its canvas its
larger way is cropped. Such an image is a small picture padded into a
large square or framed in a thumbnail.

It is cropped to the square around that content, centred on it with a
margin of 1/32 of its span each side, and scaled to 256 pixels, so it is
drawn as large as every other application's icon. A frame's rings are
left out of it (transparent, or the frame's fill).

An image's content is its visible pixels (not almost transparent), except
within a frame. A frame is a canvas of at least 64 pixels whose ring 1/16
of its side in is of one fill, transparent or light, with rings of one
colour drawn from its edge in, at least one unlike the fill. Only the
middle half of each side is looked at, so rounded corners do not count.
Within a frame, the content is the pixels within those rings unlike the
fill.

An icon whose content already spans three quarters of its canvas is kept
as it is. A system's own icon grid, as macOS's, leaves less margin than
that.

### The cache

The cache is `application-icons` in Pane's cache folder, beside the
compiled extension code: `%LOCALAPPDATA%\Pane\cache`,
`~/Library/Caches/Pane`, `$XDG_CACHE_HOME/pane`. It is not extension data
and not a managed copy.

Each icon is a PNG (or a Linux theme's SVG) named by a digest of the
application's id and its fingerprint, with a dark variant beside it,
written atomically. An index (`index.json`) records when each was
extracted.

The fingerprint is the path, size and modification time of the
application's source and of the file its picture is read from, since an
update often rewrites only that file:

- Windows: a shortcut, its own icon location if it names one, and its
  target program, to which extraction falls back when that location
  yields no picture (an update rewrites the program, not the shortcut);
  an internet shortcut and its `IconFile`; a ClickOnce reference and its
  deployed program; a program alone; a packaged app's manifest and the
  logo files chosen from it.
- macOS: the bundle's `Info.plist` and its icon file, the
  `CFBundleIconFile` in `Contents/Resources` (`.icns` added when the name
  has none), else the asset catalog `Contents/Resources/Assets.car` when
  it names a `CFBundleIconName`.
- Linux: the desktop entry and the file its `Icon` resolves to in the icon
  themes now, so a theme changed or an icon installed is seen too.

A picture's file that cannot be read counts by its path.

The index's version records how the images were made: version 2 since
icons are cropped to fill their place, 3 since a framed thumbnail is
cropped to what its frame holds. An index that cannot be read, or that an
older Pane made, is deleted with every image and rebuilt, so every icon
is extracted again. An index of version 3 from before extraction times
were recorded is read as it is, every picture in it old, so nothing is
lost and it is not rebuilt. Images the index does not name are removed.

The cache holds at most 64 MiB and 10,000 icons, the least recently drawn
going first. Deleting it loses nothing but the time to extract the icons
again.

### Refreshing

A single worker thread at low priority looks at eight icons at a time.
Low priority is Windows' background mode, a lower `nice` on Linux, and
the utility quality-of-service class on macOS. macOS's background band is
not used: it throttles a thread's processor and disk for as long as
anything else is busy, which held the worker, and an icon a row on screen
was waiting for, for half a minute on a loaded Mac.

After each start the worker looks once at every application root search
lists. It extracts an icon again only when it is missing, its fingerprint
changed, or its picture is older than the refresh age, 7 days (to catch a
change no fingerprint sees). An unchanged, younger icon is drawn from the
cache and not extracted.

An icon a row on screen wants goes first. It is drawn from the cache at
once when its fingerprint has not changed (an old one is then extracted
again in the background), and extracted at once when it has.

The worker rests after each background batch, including one that only
read fingerprints, so the start's look at every application is not a busy
loop. On Linux the fingerprints read the user's icon themes (the
configuration naming the theme and each theme's `index.theme`) once for
every look within 2 seconds, not once per application.

A failed extraction is remembered until the next start, and the row keeps
its placeholder (or the icon kept from before). Disabling the
Applications extension drops its applications from the refresh with its
results. Extraction never runs on the window's thread or the extension
runtime's.

### Drawing

Asking what a row shows only looks at what the cache keeps, so typing
never waits for an icon. Until the icon is there the row draws a neutral
placeholder in the same box (the application glyph, faded, in the
secondary tone, without a tile), so nothing moves when the icon arrives.

The window draws a packaged app's light or dark variant as the launcher's
theme is (System following the system). Icons are decorative: assistive
technology reads an application's row by its title and subtitle.

Root search draws an application's icon for every indexed result whose
action opens an application, whichever extension supplied it.

### For extensions

The applications import returns an `icon` reference with each
application. An extension's list shows the application's icon by naming
it as an item's icon, `{"application": <icon>}`
([list tree](list-tree.md#icons); `Icon::application` in Rust,
`{ application: app.icon }` in JavaScript and TypeScript). Its fallback
(or a neutral placeholder) shows until the icon is there. The JavaScript
and TypeScript samples do this. The Applications extension, a root
provider, has no list of its own.

## Per platform

| | Windows ([#24](https://github.com/pane-app/pane/issues/24)) | macOS ([#25](https://github.com/pane-app/pane/issues/25)) | Linux ([#26](https://github.com/pane-app/pane/issues/26)) |
| --- | --- | --- | --- |
| Found in | Shortcuts: shell links (`.lnk`), internet shortcuts (`.url`) whose scheme has a registered handler (its key under `HKEY_CLASSES_ROOT` is marked `URL Protocol` and has a `shell` key: a game launcher's `steam://`, `com.epicgames.launcher://`) and ClickOnce application references (`.appref-ms`), in the Start menu's `%APPDATA%\Microsoft\Windows\Start Menu\Programs` then `%ProgramData%\...\Programs`, with subfolders, and without subfolders on the user's Desktop and every user's (`FOLDERID_Desktop`, `FOLDERID_PublicDesktop`, wherever the shell keeps them) and among the taskbar pins (`%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar`); then the packaged (AppX/MSIX) apps of the shell's Apps folder (`FOLDERID_AppsFolder`), such as Calculator on Windows 11 | Application bundles (`.app`) in `/Applications`, `/System/Applications` and `~/Applications`, and their subfolders two deep (such as `Utilities`), not inside bundles | Desktop entries (`.desktop`) in `$XDG_DATA_HOME/applications` (default `~/.local/share/applications`) then `applications` in each of `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`), with subfolders (Flatpak and Snap add their folders to `XDG_DATA_DIRS`) |
| Name | The name Explorer shows for the shortcut (localized), else its file name; a packaged app's display name | The bundle's display name as Finder shows it (localized), else its folder name | The entry's `Name` for the messages locale (`Name[vi]`), else the plain `Name` |
| Also found by | The shortcut's file name, other shortcuts' names, a shell link's target program's name (an internet shortcut or a ClickOnce reference has no program) | The bundle's folder name | The plain `Name`, the `Exec` program's name, `Keywords` for the locale |
| Icon | The shell's 256-pixel image, padded or framed small icons rejected for the shortcut's icon location, its target's image, the target's own largest icon or the file information icon (an internet shortcut's `IconFile`); a packaged app's manifest logo with light and dark variants | The workspace's icon of the bundle | The entry's `Icon` in the user's icon theme, its parents, `hicolor`, then `pixmaps` |
| Identified by | A shell link's target and arguments, version folders wildcarded (an MSI-advertised one's installed program); an internet shortcut's URL; a ClickOnce reference's deployment; a packaged app's package family | The bundle identifier | The desktop file id |
| Left out | The Startup folders (`Startup` in either Start menu's Programs folder); uninstallers: a shortcut whose name contains "Uninstall" or whose program's name starts with `unins` (`unins000.exe`, `uninstall.exe`); a shell link whose target is missing (broken), empty (a shell item, or an advertised product that is not installed), a folder, or a document rather than a program (a program is `.exe`, `.com`, `.bat`, `.cmd`, `.msc`, `.cpl`, `.vbs`, `.vbe`, `.wsf` or `.wsh`); an internet shortcut to a web page or a document (`http`, `https`, `ftp`, `file`, `mailto`, `news`) or to a scheme nothing handles; folders that are symbolic links or junctions, which are not walked; a shortcut at the same place in the all-users menu (or Desktop) as in the user's; Apps folder items that are not packaged apps (desktop programs, found by their shortcuts) or that have a shortcut's name | Nothing | `Type` other than `Application`, `NoDisplay` or `Hidden` (a hidden entry also hides a lower one with the same desktop file id), no `Exec`, `OnlyShowIn`/`NotShowIn` against `$XDG_CURRENT_DESKTOP`, a `TryExec` program that is missing, an `Exec` line using field codes against the spec (`%i`, `%F` or `%U` inside an argument, more than one of `%f %u %F %U`, an unknown code or a lone `%`: skipped with a line on standard error, not guessed) |
| Opened by (the primary source) | `ShellExecuteEx` on the shortcut, or on `shell:AppsFolder\<AppUserModelID>` for a packaged app, as Explorer opens them (errors returned, no dialog), with COM initialized for the call and uninitialized after | `/usr/bin/open` on the bundle (Launch Services); its error message is shown | Running the `Exec` program directly (quoting and field codes per the Desktop Entry spec; file and URL codes dropped; `Path` as working folder), in its own process group; a `Terminal=true` entry runs in `$TERMINAL -e`, else the first installed of `x-terminal-emulator -e`, `gnome-terminal --`, `konsole -e`, `xfce4-terminal -x`, `alacritty -e`, `kitty`, `foot`, `xterm -e`, and is refused with an explanation when there is none |
| Not supported yet | Windows Settings pages and Control Panel items (a later slice of #124) | Spotlight-only locations, program names | D-Bus activation, desktop actions, `GenericName` |
| Not looked for | Programs known only to their uninstall records (winget installs), game launchers' libraries beyond the `.url` shortcuts they make, folders the user chooses for portable programs | The Desktop, the Dock, internet shortcuts (`.webloc`) and aliases, Spotlight-only locations and System Settings panes: only the bundles in the three Applications folders | The Desktop (`~/Desktop` launchers), panel or dock pins, internet shortcuts (`Type=Link` entries are left out), and D-Bus activation: only the desktop entries in the `applications` data folders |
| Desktop baseline | Windows 10/11 desktop; CI runs Windows Server 2025 (`windows-2025`) | macOS 15 (`macos-15`, arm64) | freedesktop Desktop Entry 1.5 on any desktop; run on X11 (Xvfb) only, Wayland untested |

The same author-facing contract serves all three systems; only the
adapter differs. An extension receives `application` records and returns
`open-application(id)`. A record holds:

- `id`, its stable identity;
- `name` and `location`, its primary source's;
- `alternate-titles`, `keywords` and `distinction` ([names](#names));
- `icon`, the reference that shows its own [icon](#icons).

## Checks

- Launcher public interface. These checks left Pane's repository with the
  default extensions' sources (#285) and live in the
  [Applications repository](https://github.com/pane-app/applications)
  now. They run the real Applications guest over a fake system, so which
  applications exist and what opening does are deterministic, and cover:
  - typing a name lists it and Enter opens it;
  - ranking with a command by title;
  - a blank query lists none and starts nothing;
  - looked for once per visit of root search and found again after coming
    back;
  - typing lists commands while the system is still being asked;
  - opening and listing failures explained;
  - disabling removes the applications, stops the instance and asking
    while the calculator still answers, and enabling brings them back;
  - an answer arriving after disabling discarded;
  - no "Applications" row, each application still found
    ([`root_providers.rs`](../crates/pane-core/tests/root_providers.rs)
    covers providers as such);
  - a package declaring `indexedResults` without the interface refused at
    install;
  - the JavaScript and TypeScript author examples
    ([`guests/sample-applications-js`](../guests/sample-applications-js),
    [`-ts`](../guests/sample-applications-ts)), which supply
    "Launch <name>" for each application and whose commands list and open
    them through the import, errors included.
- Identity through the launcher
  ([`crates/pane-core/tests/application_identity.rs`](../crates/pane-core/tests/application_identity.rs)),
  with the real guest and the host's list over a fake system's sources:
  - two shortcuts to one program are one result, opened by the preferred
    one;
  - different arguments stay two results;
  - an application updated into a new version folder keeps its id and its
    pin across a restart;
  - pins made before identities (a data folder an earlier Pane wrote)
    resolve and are rewritten with the new ids, and a gone one keeps its
    slot and says why;
  - the host opens an application by its old path;
  - the JavaScript and TypeScript samples receive the stable id and open
    by it.

  The pure rules (version folders and what is not one, the key and its
  digest, packaged apps' keys, primary election, legacy lookup) are unit
  tests of `identity`, and reading `Info.plist` (XML and binary) of
  `plist`.
- Live list
  ([`crates/pane-core/tests/application_cache.rs`](../crates/pane-core/tests/application_cache.rs)),
  over a fake system whose sources and watcher the test drives, on a
  manual clock:
  - nothing is scanned or watched until asked, then once;
  - a burst of reported changes is rescanned once after the debounce and
    told once, and a rescan finding the same applications tells nothing;
  - a removed application stays for its grace by the clock and then
    leaves, and one reinstalled within it never does;
  - lost changes rescan at once, a completing change again later, and the
    period reconciles what was never reported;
  - a failed rescan keeps the list and a failed first scan is an error;
  - releasing or dropping the list stops watching, and the next request
    scans and watches again;
  - a partial watch is made again after a rescan;
  - opening finds the application by its id or its old path (a scan that
    is not kept when nothing is), and an id no application has is
    explained.

  The pure rules (the grace from the first miss, a retargeted shortcut, a
  renamed one, telling a sleep, judging watcher events, the folder watched
  for a missing one) are unit tests of `cached` and `watching`.
- Live list through the launcher
  ([`crates/pane-core/tests/application_changes.rs`](../crates/pane-core/tests/application_changes.rs)),
  with the real guest and the host's list over a fake system:
  - nothing is watched until root search is first used;
  - an application installed (a program, or a packaged app whose change
    completes later) while root search is on screen appears within about a
    second without leaving it;
  - an uninstalled one leaves after its grace on the launcher's clock, and
    one reinstalled within it never does;
  - a renamed shortcut changes the title and keeps the pin;
  - a lost change and the period reconcile;
  - the selected row stays on its application, or at its position when it
    left;
  - a change while the query is blank is listed by the next query of the
    same visit;
  - showing root search again and again with no application change asks
    the Applications provider for nothing, an application change asking it
    again (#202);
  - disabling Applications stops every watcher and drops the list,
    enabling it looking and watching again.
- Names through the launcher
  ([`crates/pane-core/tests/application_names.rs`](../crates/pane-core/tests/application_names.rs)),
  with the real guest and the host's list over a fake system's sources:
  - a localized title is listed and found, and the untranslated and
    program names find it showing that title;
  - `code` and `wt` find their applications while `launcher` and `setup`
    find nothing, a program name two applications share finds neither, and
    a browser's name finds the browser but not its web app;
  - a desktop entry's program and keywords find it;
  - same-name applications get distinguishing subtitles (program name,
    else folder) while one alone keeps "Application";
  - a pin sharing its title carries the distinction (`QuickSlot::detail`)
    and one alone none;
  - the JavaScript and TypeScript samples give their results alternate
    titles and keywords that find them.

  The pure rules (generic names, program names, arguments, alternate
  titles and sharing, keywords, distinctions, locale keys and matching)
  are unit tests of `names`. The display name and `Name[..]`/`Keywords[..]`
  reading are tested in `start_menu`, `app_bundles` and
  `desktop_entries`, and matching alternate titles and keywords in
  `search`.
- Icons through the launcher
  ([`crates/pane-core/tests/application_icons.rs`](../crates/pane-core/tests/application_icons.rs)),
  with the real guest, the host's list over a fake system and a fake
  extraction:
  - a row shows its application's own icon, and the placeholder until it
    is extracted, decorative;
  - a packaged app keeps its light and dark icons, from the package even
    when a shortcut is its primary source;
  - after a restart the kept icon draws without extracting, and an
    unchanged application is not extracted again, however often root
    search lists it;
  - a changed source is extracted again at once and its old image removed,
    and in the background after a restart (a shortcut whose target was
    updated, by its fingerprint);
  - a picture past the refresh age (a test clock) is extracted again in
    the background while a row draws it from the cache at once, and is
    young again after;
  - an index from before extraction times draws its pictures at once,
    without a rebuild, and extracts them again in the background,
    recording their time;
  - a failure keeps the placeholder and is not tried again that start;
  - an unreadable cache is rebuilt and stray images removed;
  - the cache is bounded by count and by bytes, the least recently drawn
    going first;
  - a row on screen goes before the background refresh;
  - a pinned application's slot shows its icon;
  - disabling the extension stops the refresh;
  - the JavaScript and TypeScript samples show an application's icon by
    the import's reference, and the host gives that reference and the
    icon's source.

  Unit tests of `icons`, `appx`, `theme` and the Windows extractor cover:
  the visible-content check; cropping a padded icon to fill its place
  (centred, scaled up and down, a filling icon and an empty one left
  alone); finding a framed thumbnail's content and cropping it without the
  frame (a synthetic copy of Windows' frame, and an opaque light one; a
  coloured plate, a white plate and a dark fill not taken for a frame);
  the batch order; a fingerprint following the file the picture is read
  from; the refresh age; an index without extraction times read with every
  picture old; a bundle's icon file named by its `Info.plist`; the
  manifest's logo and its variants; the icon theme lookup and the file a
  desktop entry's icon resolves to; and an internet shortcut's icon file.
- Icon adapters
  ([`crates/pane-core/tests/application_icon_adapters.rs`](../crates/pane-core/tests/application_icon_adapters.rs)):
  - on every system, a desktop entry's icon is found in a fixture
    `hicolor` theme on the data folders, at the size closest to 256, after
    a theme that is not there;
  - on Windows, the command interpreter's icon extracts at 256 pixels
    filling its box; a shortcut whose own icon file holds only a 16-pixel
    image is drawn by a fallback filling its box, and its fingerprint
    follows the shortcut; a shortcut's fingerprint changes when its target
    program is updated in place, the shortcut untouched, and, with an icon
    location of its own, when that icon file or the target changes; an
    inbox packaged app (Calculator or Settings) yields its light and dark
    logos, its fingerprint covering its manifest and the logo drawn;
  - on macOS, Calculator's bundle icon is at least 256 pixels, its
    fingerprint covering its `Info.plist` and its icon file in
    `Contents/Resources`.
- Window icons
  ([`crates/pane/tests/application_icons.rs`](../crates/pane/tests/application_icons.rs)):
  - an application's row draws the placeholder bare while its icon is held
    back, then its own icon in the same box, the dark file in the dark
    theme and the light one in the light;
  - Pane's own row keeps its tile;
  - a pinned application's slot draws its icon;
  - assistive technology reads the row by its title and subtitle.
- Window ([`crates/pane/tests/window.rs`](../crates/pane/tests/window.rs)):
  - typing a name renders the application's row, which assistive
    technology sees as the selected `ListBoxOption`, and Enter opens it
    with the field keeping focus;
  - a pinned application sharing its name with another has its
    distinction as its tile's tooltip and its accessible description.
- Adapters
  ([`crates/pane-core/tests/application_adapters.rs`](../crates/pane-core/tests/application_adapters.rs)):
  - Discovery of each system's fixtures runs on every system (precedence,
    hidden and filtered entries, subfolders, uninstallers, bundles inside
    bundles, `Exec` lines against the spec). `Exec` parsing, field-code
    checks, choosing a terminal emulator and adding the Apps folder's
    packaged apps without duplicating shortcuts have unit tests.
  - Identity on every system: shortcuts (read by a fake reader) to one
    program in two version folders are one application, opened by the
    user's own; different arguments two; a shortcut on the Desktop and in
    the Start menu to one program is one application, opened by the
    Desktop's; every user's Desktop and the taskbar pins are found, and a
    Desktop's subfolder is not looked into; internet shortcuts to a scheme
    with a (fake) handler are found, one link on the Desktop and in the
    Start menu once, while those to a scheme nothing handles or to a web
    page are not; a ClickOnce reference in UTF-16 is found by its
    deployment; the Startup folders, uninstallers by name and by program,
    broken shortcuts (a fake disk), and shortcuts to folders and to
    documents are left out.
  - The macOS and Linux sources Pane does not have are stated in this
    page's "Not looked for" row.
  - An MSI-advertised shortcut the installer resolved is one application
    with a plain shortcut to its program, and one whose product is not
    installed is left out (unit tests, with the parsing of `.url` and
    `.appref-ms` files and the program, uninstaller and scheme rules).
  - An unreadable shortcut is keyed by its path; the host's list finds an
    application by its id and by a source's old path; bundles are keyed by
    their identifier (a moved copy is the same application) and desktop
    entries by their desktop file id; a shortcut to a packaged app is that
    app (unit test).
  - Names on every system: shortcuts (a fake reader giving Explorer's
    names) titled by their localized name and found by their file and
    program names, generic and argument-carrying ones not; same-name
    shortcuts told apart by their programs' folders; bundles titled by a
    (fake) display name and found by their folder name; desktop entries
    named and found for a `vi_VN` locale (`Name[vi]`, `Keywords[vi]`, the
    `Exec` program, a web app's browser left out).
  - On Windows: shortcuts made with `WScript.Shell` to a copied program in
    two version folders are read by the shell as one application, and
    inbox packaged apps are keyed by their package family; a folder's
    `desktop.ini` `LocalizedFileNames` titles its shortcut as Explorer
    shows it; the native list looks in both Desktops and the taskbar pins
    without their subfolders; a `.url` to a scheme the test registers
    under the user's classes is found and one to an unregistered scheme is
    not; an `.appref-ms` is found; and shortcuts the shell makes to a
    removed program, a folder, a text file and an uninstaller copy are
    left out.
  - On macOS, Calculator is identified by `com.apple.calculator` and found
    by its folder name.
  - On its own system each adapter opens a harmless application the test
    makes, which writes a marker file: a desktop entry (Linux), a bundle
    whose program is a shell script (macOS), a shortcut to `cmd.exe` made
    with `WScript.Shell` (Windows).
  - The native lists are also read on each system: on macOS they must
    include Calculator, on Windows an inbox packaged app (Calculator or
    Settings) from the Apps folder.
  - Each adapter's watcher runs on every system with its native watcher: a
    shortcut made in a watched Start menu subfolder or Desktop folder, a
    desktop entry, or a bundle in a temporary folder given to the adapter
    is reported within two seconds, and an `applications` folder that does
    not exist yet is reported when it appears.
- Native GUI smokes, one identical phase on all three systems
  (screenshots 44 and 45): install the package, type "pane smoke", check
  the selected row, Enter, and check that the application the smoke added
  wrote its marker file and that the launcher closed. The smoke adds it
  in a data folder, HOME or APPDATA of Pane's own, so the system's are
  searched too. See the
  [Linux](platforms/linux.md#applications-24-25-26),
  [macOS](platforms/macos.md#applications-25) and
  [Windows](platforms/windows.md#applications-24) notes for where it has
  run.

## Limits

- No aliases: the user's aliases ([#31](aliases.md)) are for installed
  commands only. No frequency ranking. Applications are not ranked
  against commands beyond the title rank, and a match on an alternate
  title or keyword highlights nothing in the row. Scoring and showing them
  better is "Root search like Raycast",
  [#122](https://github.com/pane-app/pane/issues/122).
- A Windows shortcut's localized name is read with what it opens and kept
  until the shortcut file changes. So a changed `desktop.ini` or display
  language shows once the shortcut is read again (the live list,
  [#171](https://github.com/pane-app/pane/issues/171), rescans). On Linux
  the locale is read once, when Pane starts.
- The host imports are synchronous. The very first scan runs on the
  runtime thread, so a guest call made meanwhile waits for it (later scans
  run on the list's own thread; #29 owns cancellation). A change between
  the first scan and the moment the watchers start is found by the next
  rescan (a later change, or the 30-minute period).
- Packaged apps are followed through the `Packages` folder, not the
  system's packaged-app catalog events (`PackageCatalog`). The
  specification asked to check those first for an unpackaged process, and
  Pane does not use them yet. A Store install appears once its folder is
  made and is looked at again five seconds later. A registration that
  takes longer is found by the next change or the period.
- On Windows a scan reads every shortcut through the shell, which takes
  longer than listing the folders did (Raycast measured 0.6 to 1.5 s for
  about 115 applications). Later scans read only the shortcuts that
  changed.
- A shell link the shell cannot read is keyed by its own path, as before,
  and listed. A pin made before identities is carried over only while its
  source still exists.
- Opening a ClickOnce reference starts the ClickOnce installer's own
  checks, which may show its dialog. A `.url` opens through the handler
  the system has for its scheme.
- Pane does not hide or reset after opening an application; root search
  stays as it was.
- Icons:
  - A shortcut's own icon location is extracted at 256 pixels as the
    system scales it from the closest size the file has, so a small icon
    there is drawn enlarged rather than padded.
  - A program that ships only a small icon is drawn on Windows by that
    icon as the system scales it up from the program's resources (blocky
    or soft as the system scales it). Elsewhere it is cropped and scaled
    up to fill its place, a little soft. An SVG icon (a Linux theme's) is
    kept as it is, not cropped.
  - Pixels almost transparent (alpha up to 16 of 255) count as empty when
    cropping, so a faint glow beyond the picture is cut.
  - A frame is recognised only by its shape. An icon that is itself a
    light plate drawn to its canvas's edges with a thin outline of another
    colour, and a picture spanning less than three quarters of it, is
    taken for a framed thumbnail and cropped to that picture.
  - A packaged app whose manifest logo ships no larger target size than 44
    or 48 pixels is kept at that size (drawn scaled up).
  - A ClickOnce reference's deployed program is found by the store's
    folder names alone. A deployment installed elsewhere (a machine-wide
    or online-only one) is drawn by the shell's image of its reference.
  - While no list is kept (no package asked for the applications yet, as
    when only a pinned slot shows one), finding what an icon is extracted
    from scans the system's folders without keeping or watching them.
  - The first start, or the first after an upgrade from a Pane without
    extraction times, extracts every application's icon once in the
    background (Raycast measured about 7 seconds for about 125 icons). The
    timing on Pane's runners is to be recorded with the resource
    measurements.
  - Every later start reads each listed application's fingerprint (on
    Windows a shortcut is read through the shell for its icon location and
    target) and extracts only what changed or grew old. So the pictures of
    the applications whose icons did not change are extracted again about
    once a week.
  - A change that touches neither the source nor the file its picture is
    read from (a shell icon handler drawing differently, a Linux theme's
    file replaced with one of the same size and time) shows within the
    refresh age, or at once after deleting the cache.
- The adapters trust the host's own listing: `open` accepts any existing
  shortcut, bundle or desktop entry path, which any trusted extension
  could pass (Q9's trust model).

## Tracking

Added for [#24](https://github.com/pane-app/pane/issues/24) (Windows),
[#25](https://github.com/pane-app/pane/issues/25) (macOS) and
[#26](https://github.com/pane-app/pane/issues/26) (Linux): US03, US05,
US12, US44; T01, T03, T22; contributions to G2 and G7, not claims that
they pass. The slices of "Applications done properly"
([#124](https://github.com/pane-app/pane/issues/124)) added, in order:
stable identities ([#169](https://github.com/pane-app/pane/issues/169));
localized names, alternate titles, keywords and same-name subtitles
([#170](https://github.com/pane-app/pane/issues/170)); the live list
([#171](https://github.com/pane-app/pane/issues/171)); their own icons
([#172](https://github.com/pane-app/pane/issues/172)); and the Desktops,
taskbar pins, internet and ClickOnce shortcuts as sources on Windows
([#173](https://github.com/pane-app/pane/issues/173)).
