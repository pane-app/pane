# Prioritize extension capability with Pi-style trust and distribution

**Subsequent scope decisions:** the trust/distribution choice remains accepted. The later [WASI3 requirement](0013-require-wasi-03.md), [native-helper decision](0014-optional-native-extension-helpers.md), and [current decision index](../current-decisions.md) supersede the original then-open runtime/language questions.

Accepted 2026-09-27. Extensions are trusted local code with the launcher's OS-level access; the user's explicit priority is "HARDENING IS NOT A PRIORITY, PLUGIN CAPABILITY IS". Choose Pi-style authoring and distribution through our own SDK and npm, Git and local sources, without requiring a reviewed central store or a per-extension permission sandbox.

This supersedes the proposed mandatory capability-grant model, permission-expansion approval flow, built-artifact-only installation and restriction on native helpers. Filesystem access, networking, subprocesses and native integrations are legitimate extension capabilities, subject to the OS and available runtime. WIT/Wasm remains an evaluated interoperability option, not a required security boundary; runtime, language rollout and process layout remain open. Pi-style distribution does not imply unchanged Pi or Raycast API compatibility.

Lifecycle management, cancellation, reload cleanup and low resource usage remain product requirements. They serve reliability and predictable disabling, rather than containing hostile extensions. The accepted disable and data behavior is recorded in [extension policies](../extension-policy-proposal.md).
