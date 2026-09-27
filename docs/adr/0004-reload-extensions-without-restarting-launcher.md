# Reload affected extensions while keeping the launcher open

Accepted 2026-09-27 in Q14. Development saves trigger a rebuild and reload of the affected extension, with a manual reload option; the GPUI CE launcher stays open. A compilation/build failure preserves the currently working extension and displays diagnostics; after a successful build, clean up the old extension instance and start its replacement.

Saved data survives reload. Temporary extension state is restored only through explicit extension support, rather than requiring arbitrary live objects or running tasks to survive. JS/TS and Rust share this lifecycle, with Rust compilation preceding replacement. This combines Raycast's development workflow with Pi-style explicit cleanup; it does not adopt either product's runtime implementation.

Q31 subsequently settles replacement startup failure: clean up host-managed work, report failure with Retry/logs, and do not automatically restore the older version. Normal save/build/reload can recover after a fix. Keeping working code after a build failure remains distinct from recovery after new code executes; no automatic reversal of data migrations or external side effects is promised. Detailed transport, cleanup and process-restart mechanics remain open. See [extension policies](../extension-policy-proposal.md) for the product contract.
