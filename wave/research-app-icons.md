# Pane app and file icons on Windows: diagnosis and research (2026-10-24)

## What the user saw

Pane 0.1.1, right after the in-app update from 0.1.0. When the user searched "chrome", Google Chrome showed the grid placeholder and most file rows showed the document placeholder. Later, scrolling the app list, Computer Management, Core Temp, Debuggable Package Manager, Defragment, the VS 2022 prompts and Discord still showed the placeholder. A little later, Chrome's real icon appeared.

## Root cause: Wasmtime's cache cleanup deletes Pane's own caches

- `crates/pane-core/src/runtime.rs` `engine()` (about line 2122) passes Pane's whole cache folder (`%LOCALAPPDATA%\Pane\cache`) to Wasmtime as its code cache directory: `CacheConfig::with_directory(cache_dir)`.
- Wasmtime treats that directory as its own. Each time it writes a newly compiled module, its cache worker runs a cleanup, at most once per `cleanup_interval` (1 h by default), and marks it with a `.cleanup.wip-<pid>` lock.
  - The cleanup lists the tree (`wasmtime-internal-cache-49.0.1/src/worker.rs`, `list_cache_contents`) and deletes every file it does not recognise, "remove unrecognized files anyway".
  - A file directly inside a first-level folder counts as unrecognised. That covers `application-icons/*.png`, `application-icons/index.json`, `file-index/0.wal` and `file-index/index.json`.
- On the user's machine:
  - At 10:53:54 the 0.1.1 process (pid 51960) compiled the updated Calculator extension (package dir `10`, created 10:53:54) and left `cache\.cleanup.wip-51960` (created 10:53:54).
  - The `application-icons` folder still has its creation time of Oct 8, but every image in it and its `index.json` were extracted after 10:56 (index `tick` started at 0).
  - `data\extensions\system-icons` (file icons), which lives in the data folder, kept its PNGs from Oct 23.
- Consequences:
  - Every app icon is extracted from scratch after any extension install or update, or a new Pane build that compiles modules.
  - The file index is deleted too and rebuilt, which keeps the disk busy and slows the icon extraction running beside it.
- 0.1.0 has the same bug. The code is unchanged since before v0.1.0; the update only triggered it, because the Calculator update compiled a module.

## Why extraction then took so long (contributing causes)

- The app-icon worker runs at `THREAD_MODE_BACKGROUND_BEGIN` (`applications/icons.rs` `lower_priority`). That mode also gives the thread very low I/O and memory priority.
  - Each icon is written with fsync and a folder sync (`atomic::write_atomically`).
  - While the file index was being rebuilt, single icons took 10–27 s: Computer Management 27.4 s, Core Temp 27.4 s, Debuggable Package Manager 10.4 s, dfrgui 13.3 s.
  - In the same session other icons took 50–100 ms, and on an idle disk all of them do.
  - The docs already record that macOS's background band was dropped for the same reason.
- Every extraction calls `CoInitializeEx`/`CoUninitialize` around itself (`windows_shell::Com` per call). `CoUninitialize` unloads the shell handler DLLs each time, so they are reloaded for every icon.
- A row on screen waits for the current batch of 8 background icons to finish before its own icon is extracted.
- Root search's indexed results, and so the icon cache's list of apps, are asked for only when root search is first shown, not at start. On a cold cache, extraction starts when the user opens Pane.

## Measurements (this PC, 115 apps, dev build of `crates/pane-core/examples/icon_survey.rs`)

- Extraction quality: 115 of 115 apps get a real icon. None failed, and none fell back to a padded small icon.
- Extraction speed, normal priority, COM set up per call: 3.7 s total; p50 29 ms, p90 55 ms, max 95 ms.
  - Background mode on an idle disk: 3.6 s.
  - COM held for the thread's life: 3.2 s (p50 25 ms).
- Filling the cache through `IconCache` (debug build, PNG crop and encode included):
  - From empty: 115/115 shown after 9.1 s.
  - Restart with the cache kept: 115/115 shown after 0.35 s, nothing extracted again.
- So a cache that survives draws every app at once after a restart. The wipe is what made the user wait.
- File icons: `.go` and `.png` rows extract in 15–110 ms each (`NativeIcons`).

## How other launchers do it (from a research pass; links are the sources)

- **PowerToys Run** (`Wox.Infrastructure/Image/WindowsThumbnailProvider.cs`, `ImageLoader.cs`, `ImageCache.cs`, https://github.com/microsoft/PowerToys/tree/main/src/modules/launcher):
  - Extraction: `IShellItemImageFactory::GetImage` at 64 px with `SIIGBF_ICONONLY` (thumbnail only for `.png`). `.lnk` goes through `Icon.ExtractAssociatedIcon`.
  - Threads: thread-pool (MTA) tasks.
  - Cache: in memory, keyed by path. Usage counts are saved as JSON, and those paths are extracted again at startup.
  - Packaged apps: the manifest's `Square44x44Logo` with scale and targetsize variants (`Microsoft.Plugin.Program/Programs/UWPApplication.cs`).
- **Command Palette** (PowerToys PR #50192, https://github.com/microsoft/PowerToys/pull/50192, `Microsoft.CmdPal.UI/Helpers/Icons/*`):
  - Lookup: `SHGetFileInfo(SHGFI_SYSICONINDEX)`, with a per-type fallback through `SHGFI_USEFILEATTRIBUTES`.
  - Pixels: `SHGetImageList(SHIL_JUMBO)`, cached by image-list index, so files of one type share one bitmap. A per-extension placeholder shows first (not for `.lnk`).
  - Index cache: path → index, 8192 entries for 60 min, cleared on `SHCNE_ASSOCCHANGED`.
  - Large app icons: `GetImage` with `SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK | SIIGBF_CROPTOSQUARE`.
  - Threads: `clamp(cores/2, 1, 4)` workers; visible icons first, and one worker kept free for them.
  - Sizes: separate caches for 20, 64 and 256 px, all in memory.
- **Flow Launcher** (`Flow.Launcher.Infrastructure/Image/ThumbnailReader.cs`, `ImageLoader.cs`, `ImageCache.cs`, https://github.com/Flow-Launcher/Flow.Launcher):
  - Extraction: `GetImage` at 64 px, with `IconOnly` for folders and a retry with `IconOnly`, then `ExtractAssociatedIcon`. For `.url`, the `IconFile=` it names.
  - Cache: in-memory LFU of 150 entries. The keys are saved to `Image.cache` and extracted again in the background at startup; no pixels are stored.
  - Packaged apps: the `Square44x44Logo` asset closest to 64 px.
- **Raycast for Windows**: nothing public on how it extracts or caches icons. The changelog only mentions light/dark app icons, a fix for shortcuts whose icon file is gone, and lower icon memory (https://www.raycast.com/changelog/windows).
- **Windows itself**:
  - Per-type vs per-file icons: a file type has one icon unless its `DefaultIcon` is `%1` or it has `shellex\IconHandler` (https://learn.microsoft.com/en-us/windows/win32/shell/how-to-create-icon-handlers).
  - On this PC, per-file types are `exefile`, `icofile`, `curfile`, `anifile`, `MSCFile`, `lnkfile`, `InternetShortcut` (`.url`), `Application.Reference` (`.appref-ms`), `.website` and `.library-ms`. Types such as `.go`, `.pdf`, `.txt`, `.png` (icon only), `.dll` and `.msi` have one icon per type.
  - The icon cache lives in `iconcache_*.db`. `SIIGBF_INCACHEONLY` reads only the shell's cache (https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ishellitemimagefactory-getimage).
- **COM and priority**:
  - `CoUninitialize` "unloads all DLLs loaded by the thread" (https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-couninitialize). The shell wants STA threads (https://devblogs.microsoft.com/oldnewthing/20101118-00/?p=12253).
  - `THREAD_MODE_BACKGROUND_BEGIN` lowers disk and memory priority as well as CPU priority (https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadpriority).
- What Pane already does better: none of these launchers keeps icon pixels on disk. Pane's on-disk cache, once it is not wiped, draws every app at once after a restart.

## Duplicates (Discord listed twice): recommendation only

- The user's shortcuts:
  - `Desktop\Discord.lnk` → `...\Discord\Discord.exe` with arguments `--processStart Discord.exe`
  - `Start Menu\Programs\Discord.lnk` → `...\Discord\Discord.exe` with no arguments
  - `Start Menu\Programs\Discord Inc\Discord.lnk` → `...\Discord\Discord.exe --processStart Discord.exe`
  - Taskbar pin → `...\Discord\Update.exe --processStart Discord.exe`
- Pane keys a shortcut by target and arguments (`applications/start_menu.rs` `link_key`), so the first two get two identities.
- How others dedupe:
  - PowerToys Run and Command Palette compare (name, executable name, resolved target path) and ignore arguments (`Win32Program.cs`).
  - Flow Launcher groups by target path plus arguments (`Programs/Win32.cs`).
- Recommendation for Pane's discovery (pane-core `start_menu.rs`, not the applications extension repo):
  - Group shortcuts by their `System.AppUserModel.ID` when they carry one. Desktop apps set it too, for example Squirrel's `com.squirrel.Discord.Discord`.
  - Otherwise group by the resolved target program when the shortcuts share a display name, so different arguments to the same program with the same name are one app. The Start Menu shortcut stays primary.
  - Map a Squirrel `Update.exe --processStart X.exe` to its sibling `X.exe`.
  - This needs its own ticket: it changes ids, which quick slots and learned rankings carry over through the existing id carry-over.

## Before and after the fix (PR #489, issue #488)

The A/B test runs the old and new `pane-core` through `IconCache` with the native extractor, on the 115 apps here and a fresh cache folder each time.

| | Old | New |
|---|---|---|
| Idle disk, empty cache | 5.2 s, 18 s, 37.7 s and 62.9 s (noisy: other builds were running) | 3.8 s, 4.6 s, 6.4 s and 7.1 s |
| Another process writing and fsyncing 16 MB at a time, empty cache | 46.3 s, 61.6 s and 60.8 s | 5.5 s and 14.1 s |
| Restart with the cache kept | — | 0.26–0.35 s for all 115 |

- With the earlier per-image fsync still in place, the new priority alone did not help under the disk load (51–73 s). The flushes were the main cost under load.
- Discord's three shortcuts all carry `System.AppUserModel.ID = com.squirrel.Discord.Discord`, which confirms that grouping by AUMID would merge them.
- The survey tool used for these numbers is kept at `C:\Users\ADMIN\Desktop\nguyenvu\icon-measure\scripts\icon_survey.rs`. It is not committed.
