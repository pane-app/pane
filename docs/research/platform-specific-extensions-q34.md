# Platform-specific extensions and features (Q34)

2026-09-27. Q34 subsequently accepted with the user condition "if its not too complex": supported-OS declarations and simple action checks/reasons, without an elaborate compatibility framework. Q35 separately defers catalog decisions until the launcher and extension system work.

## Raycast

Yes: the extension manifest has a `platforms` array listing macOS and/or Windows. Its documentation explicitly instructs authors to restrict installation when using platform-specific APIs. The documented command-properties table does not expose an equivalent per-command `platforms` field, so do not claim that mechanism exists at both levels. Per-platform preference defaults and shortcuts are also documented. [Manifest](https://developers.raycast.com/information/manifest), [Windows API changelog](https://developers.raycast.com/misc/changelog).

For functionality needing an installed app or CLI, Raycast's guidance recommends a helpful missing-dependency message, or making just the dependent actions available when that dependency exists. This is runtime feature availability, distinct from whole-extension platform eligibility. [Best practices](https://developers.raycast.com/information/best-practices#handle-runtime-dependencies).

## VS Code

Yes: authors can publish platform-specific VSIX artifacts, including OS and CPU architecture, with the matching artifact selected for the user. An untargeted package can provide a universal fallback. This is useful for native dependencies. The same documentation recommends conditional `when` clauses for features unavailable in web environments rather than separate manifests. [Publishing documentation](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#platform-specific-extensions).

## Zed

The extension API exposes `current_platform()` returning operating system and architecture, enabling extension code to select a supported native helper or handle unsupported environments. This establishes an author platform-detection mechanism; it does not establish a Raycast-equivalent manifest installation filter. [Versioned API](https://docs.rs/zed_extension_api/0.7.0/zed_extension_api/fn.current_platform.html).

## Accepted direction

Allow third-party authors to declare their supported platforms. Where an extension is broadly usable but one action requires a particular OS or desktop capability, retain usable functionality and explain why that action is unavailable. Whole-extension incompatibility should be reported before activation. Exact hide/disable presentation is still open; no requirement to implement both is adopted. Core launcher support remains Windows/macOS/Linux. Native helper availability and actual host APIs determine practical support even when the component uses WASI 0.3.
