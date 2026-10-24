**Pane {version} is an unsigned preview.** Expect rough edges, and expect your system to warn you before it runs Pane (see below).

## What's new

- **Currencies in the calculator** (#475): the calculator converts between currencies, as Raycast's does.
- **A back button on every pushed view** (#482): forms, detail views and canvases, which have no search bar, get a top bar with a back button.
- **Updates like Raycast's**: Pane checks for updates every few hours, downloads them by itself, and **Restart to Update Pane** restarts into the new version with one Enter. After an update, **What's New** shows these notes for a day.
- **Updating from 0.1.1 or 0.1.0**: once more by hand. Pane finds this release by itself: choose **Update Pane to {version}** in the launcher, then quit Pane from its tray or menu-bar icon and start it again (or run the setup or installer of this release over it). From this version on, updates restart by themselves (see Updates below).

## What was checked

- **Windows (x86_64):** checked by hand: installed and used on a Windows PC, beside the automated checks CI runs on every change.
- **macOS (Apple silicon) and Linux (x86_64):** built and smoke-tested by CI only. They are **not yet tested by hand**, so treat them as experimental.

## Download

| System | Package |
| --- | --- |
| Windows x86_64 | `Pane-Setup-{version}.exe` (the setup), or `pane-{version}-windows-x86_64.zip` (portable) |
| macOS Apple silicon | `Pane-{version}.dmg` (open it and drag Pane onto Applications), or `pane-{version}-macos-aarch64.zip` |
| Linux x86_64 | `Pane-{version}-x86_64.AppImage` (make it executable and run it), or `pane-{version}-linux-x86_64.tar.gz` |

Each download has a `.sha256` file beside it. To check a download, compare the digest it names with `Get-FileHash <package>` (Windows), `shasum -a 256 <package>` (macOS) or `sha256sum -c <package>.sha256` (Linux). Each package's `README.txt` says how to install and uninstall it.

## Getting past the unsigned-app warnings

Nothing in this release is signed: there is no Authenticode certificate for Windows and no Apple Developer ID for macOS yet.

- **Windows.** Run `Pane-Setup-{version}.exe`. It installs Pane for you alone, with no administrator rights, into `%LOCALAPPDATA%\Pane`: a Start menu entry, an entry in **Settings → Apps** to uninstall it from, and a choice to start Pane when you sign in. If SmartScreen says "Windows protected your PC", choose **More info**, then **Run anyway**. Installing it over an earlier Pane keeps your extensions and settings. For the portable zip instead, unzip the package, then run `powershell -ExecutionPolicy Bypass -File install.ps1` in the `pane` folder it unpacked; `-ExecutionPolicy Bypass` answers PowerShell's refusal of a script that came from the internet (or run `Unblock-File install.ps1` once), and SmartScreen's warning when Pane starts is answered the same way.
- **macOS.** Open `Pane-{version}.dmg` and drag Pane onto Applications (or, from the zip, run `bash install.sh` in the unpacked `pane` folder, which installs to `~/Applications`). The first time you open Pane, macOS says it cannot check it for malicious software. Choose **Done**, open **System Settings → Privacy & Security**, find the message about Pane near the bottom, choose **Open Anyway**, and confirm. (Or, in Terminal: `xattr -dr com.apple.quarantine /Applications/Pane.app`.) Pane has no Dock icon: it lives in the menu bar and opens with its hotkey.
- **Linux.** No warning. Run the AppImage with `chmod +x Pane-{version}-x86_64.AppImage && ./Pane-{version}-x86_64.AppImage`. To have it in your applications menu, open it with an AppImage integration tool such as Gear Lever or AppImageLauncher. Or run `bash install.sh` in the folder the tarball unpacked, which adds Pane to the menu itself.

## Source

Pane is free software under GPL-3.0-or-later. `pane-{version}-source.tar.gz` (with its `.sha256`) holds the source this release was built from: this tag's tree and, under `vendor/`, every crate its `Cargo.lock` pins, so `cargo build --release --offline --config vendor/config.toml -p pane` builds Pane without reaching the network. Each package's `THIRD-PARTY-NOTICES.txt` lists the third-party parts and their licences.

## Updates

Pane checks this repository's releases when it starts, every few hours and when you open it after a while, and downloads a newer version by itself, checking it against its `.sha256`. It installs it only when you choose **Restart to Update Pane**, first in the launcher (or the toast's **Restart**): Pane then restarts into the new version, keeping your extensions and settings, and shows **What's New** with the release's notes for a day. On Windows, an update installed from inside Pane also gives its entry in Settings → Apps the new version. On macOS, the update replaces the program inside Pane.app wherever you put it. On Linux, an AppImage is replaced by the new AppImage at the same path.
