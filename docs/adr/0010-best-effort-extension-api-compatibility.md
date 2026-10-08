# Favor API stability without indefinite compatibility obligations

Accepted in Q24: keep extension APIs stable as much as practical, while allowing changes when the launcher needs to evolve. Extension maintainers are responsible for adapting their packages; abandoned extensions do not oblige the launcher to preserve obsolete APIs or take over maintenance.

This replaces the proposed strict promise to preserve every stable SDK call within a major API version. Exact versioning, deprecation periods and migration tooling remain open; communicate incompatibility clearly rather than promising old extensions will always work. The policy applies to both JS/TS and Rust. See [extension policies](../extension-policy-proposal.md).

**Refined 2026-10-08 by [ADR 0046](0046-every-breaking-change-to-the-extension-api-gets-a-new-version.md):** every breaking change gets a new API version, Pane runs extensions built for the previous version for a support window, and breaking changes are announced with before-and-after code. Supporting every past version indefinitely stays rejected.
