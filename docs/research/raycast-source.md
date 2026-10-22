# Raycast extension source evidence

Inspected on 2026-09-27 using read-only GitHub CLI API requests. Repository pinned to [`ab59a3520e4055118e18953b43235bd7537ab70b`](https://github.com/raycast/extensions/tree/ab59a3520e4055118e18953b43235bd7537ab70b). No dependency installation, extension execution, or complete repository clone was needed.

## What this repository establishes

The root README describes this repository as the source of extensions available through the Raycast Store. It links authors to the developer documentation and extension submission guidelines. This is extension source, not an audit of the desktop host's implementation. [README](https://github.com/raycast/extensions/blob/ab59a3520e4055118e18953b43235bd7537ab70b/README.md)

### Extensions directly use filesystem, network, and process APIs

- **Filesystem:** Append Text to File imports `writeFile`, `rename`, and `unlink` directly from `node:fs/promises`. Its helper writes beside a target file and replaces that file using a rename. [Atomic write implementation](https://github.com/raycast/extensions/blob/ab59a3520e4055118e18953b43235bd7537ab70b/extensions/append-to-file/src/lib/atomic-write.ts)
- **Processes:** Flush DNS imports `execSync` and `execFileSync` directly from `node:child_process`. It executes `ipconfig /flushdns` on Windows and uses a separate privileged-command helper on macOS. This source demonstrates OS-specific implementation within one extension. [Command implementation](https://github.com/raycast/extensions/blob/ab59a3520e4055118e18953b43235bd7537ab70b/extensions/flush-dns/src/index.ts)
- **Network:** The GitHub extension builds GraphQL and Octokit clients and imports `node-fetch`; its download helper also calls `fetch` directly and writes downloaded content with Node filesystem APIs. GitHub OAuth scopes concern access to the user's GitHub account; they should not be confused with a host-enforced filesystem or network capability grant. [Authentication client](https://github.com/raycast/extensions/blob/ab59a3520e4055118e18953b43235bd7537ab70b/extensions/github/src/api/githubClient.ts), [Downloader](https://github.com/raycast/extensions/blob/ab59a3520e4055118e18953b43235bd7537ab70b/extensions/github/src/helpers/download.ts)

These concrete examples support an extension programming model with powerful Node APIs. Imports alone do not prove the absence of every runtime restriction; official host/security documentation must establish that broader claim.

### Manifest and development conventions

The small Flush DNS manifest declares command metadata, `mode: "no-view"`, dependencies, and supported platforms. Its scripts include `ray develop`, `ray build`, `ray lint`, and a publishing command. No filesystem, subprocess, or network permission declaration appears in that inspected manifest. This establishes the manifest convention in this example, not the complete schema. [Manifest](https://github.com/raycast/extensions/blob/ab59a3520e4055118e18953b43235bd7537ab70b/extensions/flush-dns/package.json)

Flush DNS and Bash Commands explicitly declare both `macOS` and `Windows`; Append Text to File declares only `macOS`. Supporting two operating systems in a host therefore does not automatically make every extension portable. [Bash Commands manifest](https://github.com/raycast/extensions/blob/ab59a3520e4055118e18953b43235bd7537ab70b/extensions/bash-commands/package.json), [Append Text manifest](https://github.com/raycast/extensions/blob/ab59a3520e4055118e18953b43235bd7537ab70b/extensions/append-to-file/package.json)

The `ray develop` scripts demonstrate a standard development entry point. These extension files do not establish whether reload preserves view state, how generations are disposed, or whether replacement is transactional; those require CLI documentation or runtime implementation evidence.

## Implication for this project

Recommended design inference: familiar TypeScript manifests and command contributions would reduce author friction. A restrictive permission system would be a deliberate departure from these direct Node API examples and requires an enforceable runtime boundary. A separate process alone should not be presented as that boundary. Platform declarations and OS adapters should be explicit from the first SDK version.
