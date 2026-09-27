# Raycast extension model and implications

Researched 2026-09-27 from official developer documentation, with a separate [public-source check](raycast-source.md). This is not a runtime audit of Raycast's desktop host.

Decision update: after the [broader comparison](extension-trust-comparison.md), the user chose Pi-style trusted extensions and distribution, prioritizing capability over hardening. See [ADR 0002](../adr/0002-trusted-extensions-and-open-distribution.md) and [accepted extension policies](../extension-policy-proposal.md). Runtime selection remains open; the earlier conditional Wasm capability-grant recommendation is superseded.

## Runtime and access

Raycast documents one managed Node child process, extension workers with separate V8 isolates and bounded heaps, and an RPC bridge to host APIs. It explicitly says filesystem/network/other Node operations are not additionally sandboxed. macOS privacy controls apply through the parent application. Its storage API separates extension data. This page remains macOS-focused and includes forward-looking background-execution language; it is not verification of Windows implementation details. [Raycast security documentation](https://developers.raycast.com/information/security)

**Interpretation:** execution separation and a restricted host API do not establish restrictions on what extension code can do through Node. An install-time list of requested permissions would not enforce anything if unrestricted filesystem, network or process APIs remain available.

## Distribution and review

The documented public publishing flow opens a pull request to `raycast/extensions`; review and acceptance precede publishing to the Store. That is an important part of the trust model. Local development/import and code installed from another source should not be presented as having passed that review. [Publishing process](https://developers.raycast.com/basics/publish-an-extension), [local development tools](https://developers.raycast.com/basics/getting-started)

Store rules require reproducible dependency inputs and validation builds, restrict opaque/heavy binaries, and require integrity checks for downloaded executables. They reject direct Keychain-access requests. These are publication policies, not evidence that the runtime technically prevents those operations. [Store requirements](https://developers.raycast.com/basics/prepare-an-extension-for-store)

## Developer experience and portability

Raycast describes TypeScript/React/Node authoring and hot reload as developer features. The public manifest specifies supported platforms (`macOS` and/or `Windows`), commands, preferences and other contributions. Its documented extension-specific fields do not include a general scoped filesystem/network permission manifest. [Developer API introduction](https://developers.raycast.com/), [manifest contract](https://developers.raycast.com/information/manifest)

An extension can therefore be portable at the JavaScript level while still depending on OS-specific commands or APIs. Supporting Raycast's authoring style, supporting its API, and running every existing Raycast extension are three distinct commitments.

## Recommendation for this project

General-purpose describes the user's experience. A small core describes ownership of functionality. They fit together: ordinary users get useful defaults; authors get a small API for adding and sharing functionality.

Recommended host responsibilities: palette/navigation, result dispatch/ranking, rendering primitives, extension installation/lifecycle/recovery, settings/storage, and platform integration mechanisms. Candidate default extensions: apps, calculator and quicklinks. File search and clipboard history are important candidates for the daily-use bundle, but need their own scope and platform verification. AI is confirmed as extension functionality.

The trust decision is open. Given the user's Pi-like emphasis, an explicitly trusted-code initial release is a credible simpler choice: install means granting the extension local-code trust, subject to the OS; source identity, pinned artifacts, review status, rollback and safe mode improve operational reliability. They do not convert it into a sandbox. A public catalog would need a review policy before it could be described as reviewed.

This refines the earlier restricted-by-default recommendation: enforced restrictions would be a stronger boundary, but require constraining the runtime and all host APIs and deciding what to do about subprocess escape routes. Choose that path now if it is part of the product promise. Do not ship cosmetic permission prompts and claim isolation.

Neither trust model determines whether the UI must use a browser, whether extensions need to be resident, or whether Node must always run while the launcher is hidden. Those are separate architecture and measurement questions.
