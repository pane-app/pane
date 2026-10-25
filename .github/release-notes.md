**Pane {version} is an unsigned preview.** Expect rough edges, and expect your system to warn you before it runs Pane (see below).

## What's new

- **The Windows key keeps working** (#543): on Windows, Pane's hotkeys could stop working until a restart. That happened when Pane came to the front without Windows saying so (closing a virtual desktop is one way) and you then typed into it. Now Pane checks which window is really in front, and the hotkeys never stay dead.
- **Record a double tap** (#545): double-tapping a modifier in a hotkey recorder records it ("Ctrl Ctrl"). Before, it recorded a single tap. A single tap is now recorded once the double-tap window has passed.
- **Find titles that start with punctuation** (#544): search finds `*notes.txt` by `notes`, and `[Group] Title` by `group`, in root search and in lists such as Switch Windows.
- **Double-click the Settings title bar** (#546): this maximizes the Settings window and restores it, as other windows do. On macOS it follows the system's title bar setting.
- **One Pane at a time** (#547): starting Pane while it runs (from Start, the setup or a second double-click) now shows the running Pane's launcher, as Raycast does, instead of starting a second Pane.
- **Updating from 0.1.2 or 0.1.3**: Pane finds this release, downloads it and offers **Restart to Update Pane**, or restarts into it by itself while you're away. **From 0.1.1 or 0.1.0**: choose **Update Pane to {version}** in the launcher, then quit Pane from its tray or menu-bar icon and start it again (or run the setup or installer of this release over it).

## What was checked

- **Windows (x86_64):** built and smoke-tested by CI on every change. 0.1.3 was checked on a Windows 11 PC: the Windows key and its shortcuts, taps and double taps, paste, Switch Windows, the calculator, clipboard history, the compact window, glass, the Settings window and restart persistence. That check found the five problems this release fixes. Each fix was then checked again on the same PC, with a build of this release's code.
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
