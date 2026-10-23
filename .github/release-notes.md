**Pane {version} is an unsigned preview.** Expect rough edges, and expect your system to warn you before it runs Pane (see below).

## What was checked

- **Windows (x86_64):** checked by hand: installed and used on a Windows PC, beside the automated checks CI runs on every change.
- **macOS (Apple silicon) and Linux (x86_64):** built and smoke-tested by CI only. They are **not yet tested by hand**, so treat them as experimental.

## Download

| System | Package |
| --- | --- |
| Windows x86_64 | `pane-{version}-windows-x86_64.zip` |
| macOS Apple silicon | `pane-{version}-macos-aarch64.zip` |
| Linux x86_64 | `pane-{version}-linux-x86_64.tar.gz` |

Each package has a `.sha256` file beside it. To check a download, compare the digest it names with `Get-FileHash <package>` (Windows), `shasum -a 256 <package>` (macOS) or `sha256sum -c <package>.sha256` (Linux). Each package's `README.txt` says how to install and uninstall it.

## Getting past the unsigned-app warnings

Nothing in this release is signed: there is no Authenticode certificate for Windows and no Apple Developer ID for macOS yet.

- **Windows.** Unzip the package, then run `powershell -ExecutionPolicy Bypass -File install.ps1` in the `pane` folder it unpacked; `-ExecutionPolicy Bypass` answers PowerShell's refusal of a script that came from the internet (or run `Unblock-File install.ps1` once). If SmartScreen says "Windows protected your PC" when Pane starts, choose **More info**, then **Run anyway**.
- **macOS.** Run `bash install.sh` in the unpacked `pane` folder. The first time you open Pane, macOS says it cannot check it for malicious software. Open **System Settings → Privacy & Security**, find the message about Pane near the bottom, choose **Open Anyway**, and confirm. (Or, in Terminal: `xattr -dr com.apple.quarantine ~/Applications/Pane.app`.)
- **Linux.** No warning: run `bash install.sh` in the unpacked `pane` folder.

## Source

Pane is free software under GPL-3.0-or-later. `pane-{version}-source.tar.gz` (with its `.sha256`) holds the source this release was built from: this tag's tree and, under `vendor/`, every crate its `Cargo.lock` pins, so `cargo build --release --offline --config vendor/config.toml -p pane` builds Pane without reaching the network. Each package's `THIRD-PARTY-NOTICES.txt` lists the third-party parts and their licences.

## Updates

Pane checks this repository's releases when it starts and tells you when a newer version is out. It never downloads or installs one unless you choose to, and it checks each download against its `.sha256`.
