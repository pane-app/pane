**Pane {version} is an unsigned preview.** Expect rough edges, and expect your system to warn you before it runs Pane (see below).

## What's new

- **Extensions can get your attention** (#553): an alarm, a timer or a reminder can now reach you while Pane is hidden. An extension can post a system notification, which opens one of its commands when you click it (#563). It can ring a sound until you stop it, and the sound always stops when the extension's code stops (#562). And it can bring the launcher up on its own command, waiting until you stop typing (#561). This works on Windows, and on macOS and Linux too (#566), where it has not been tried by hand yet. A package must ask for it in its `package.json`.
- **For extension authors** (#564, #567): the alerts API is in the SDKs `pane-extension` 0.2.1 and `@pane-app/extension` 0.2.1, with a sample in Rust, JavaScript and TypeScript and a guide in `docs/alerts.md`. New TypeScript extensions start on SDK 0.2.1 (#569), and the npm CLI moves to `@pane-app/cli` 0.1.3.
- **The launcher hides when you click away** (#573): on Windows, when another application, the taskbar or the desktop takes the focus, the launcher now hides, as Raycast does. Before, it stayed on screen. Pane's own Settings window and file dialogs keep it shown.
- **Missed notifications are written down** (#572): when Pane or Windows refuses or drops a notification, `pane.log` now says so and why. The notification's own text is never written.
- **Updating from 0.1.2, 0.1.3 or 0.1.4**: Pane finds this release, downloads it and offers **Restart to Update Pane**, or restarts into it by itself while you're away. **From 0.1.1 or 0.1.0**: choose **Update Pane to {version}** in the launcher, then quit Pane from its tray or menu-bar icon and start it again (or run the setup or installer of this release over it).

## What was checked

- **Windows (x86_64):** built and smoke-tested by CI on every change. The notifications, the ringing sound and the launcher coming up were checked by hand on a Windows 11 PC with a build of this release's code. That check found the two problems #572 and #573 deal with. Those two are checked by CI only.
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

Pane checks this repository's releases when it starts, every few hours and when you open it after a while, and downloads a newer version by itself, checking it against its `.sha256`. It installs it when you choose **Restart to Update Pane**, first in the launcher (or the toast's **Restart**), or by itself once Pane has been hidden for ten minutes with nothing running in it, as Raycast does. Pane then restarts into the new version (hidden, when it restarted by itself), keeping your extensions and settings, and shows **What's New** with the release's notes for a day. On Windows, an update installed from inside Pane also gives its entry in Settings → Apps the new version. On macOS, the update replaces the program inside Pane.app wherever you put it. On Linux, an AppImage is replaced by the new AppImage at the same path.
