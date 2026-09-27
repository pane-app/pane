# Native helper precedents for Q33

2026-09-27. Q33 recommendation subsequently accepted: user said "hmm ok then". See [ADR 0014](../adr/0014-optional-native-extension-helpers.md). The user's concurrent WASI 0.3 requirement is accepted separately; precedents below do not validate our p3 toolchain.

## Pi

Trusted extensions execute in Pi's own process with its OS permissions. `pi.exec(command, args, options)` is backed by Node child-process spawning, returning stdout/stderr/exit status with optional abort and timeout. Long-lived resources belong in session/command work and require idempotent shutdown cleanup by authors. This is direct native-runtime capability, not a Wasm escape hatch or a promise that all required tools are bundled. [Extension contract](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/extensions.md), [exec implementation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/exec.ts).

## Raycast

Raycast offers command execution through `useExec`. Its Store guidelines allow system binaries, externally downloaded binaries with integrity verification, and binaries copied from npm packages with traceable builds; they discourage large bundled binaries and opaque untraceable builds. The guidance explicitly favors automating downloads over manual user setup. These are Raycast Store rules, not policies automatically adopted for this launcher. [Execution utility](https://developers.raycast.com/utilities/react-hooks/useexec), [binary dependency guidelines](https://developers.raycast.com/basics/prepare-an-extension-for-store#binary-dependencies-and-additional-configuration).

Raycast also maintains Swift tooling that connects TS/React extensions to a Swift executable for native macOS APIs, generating argument/result interfaces. Xcode is listed as a developer prerequisite; this is not evidence every end user needs Xcode. [Official Swift tools](https://github.com/raycast/extensions-swift-tools).

## Zed

Zed's Wasm extension API can return a native language-server executable command, arguments and environment for the host to run. It also exposes file download/extraction into extension storage and a general process `Command::output` API. This is a closer structural precedent for a Wasm guest requesting native work through host APIs, but not evidence that arbitrary native libraries load inside Wasm, that every binary is bundled, or that all process descendants are automatically cleaned up. [Language server contract](https://zed.dev/docs/extensions/languages#language-servers), [download API](https://docs.rs/zed_extension_api/0.7.0/zed_extension_api/fn.download_file.html), [process API](https://docs.rs/zed_extension_api/0.7.0/zed_extension_api/process/struct.Command.html).

## Recommendation for our WASI 0.3 design

Allow an optional prebuilt native helper beside the component or managed as a platform-specific download. The component requests execution through our SDK/host API; the native helper can link libraries that cannot run in the guest. Supply process input/output, cancellation, ownership and lifecycle cleanup as host functionality. This follows the Wasm-plus-native-process pattern without changing the primary extension entry point or silently adding Node as a second default runtime.

Native helper artifacts still vary by OS/architecture and may need packaged dynamic libraries or runtimes. Startup/IPC and active-process memory add costs to measure; portable-only extensions need no helper. Generic helper packaging should avoid normal-user compiler setup. Automatic downloads imply first-use networking and cached subsequent use. Q33 helper support is accepted; exact bundled-versus-download policy remains open.
