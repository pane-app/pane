# Raycast root search and the launcher core boundary

Checked 2026-09-27 against current official manual/API pages. This is documentation research, not an audit of Raycast's closed-source desktop host. Q17 concerns default provider participation, not whether the launcher has core search. The user subsequently accepted Raycast-style initial search in [ADR 0006](../adr/0006-raycast-style-search-with-extension-providers.md); optional global online-provider support remains future design work.

## What Raycast does

Root Search is the main result view before opening a command. Current documentation includes apps, built-in/extension commands, quicklinks and inline calculator results. It also includes indexed files/directories when configured, plus other integrated features. The manual describes aliases, fuzzy matching and frequency/recency ranking. Do not rely on an older model that always puts files behind a separate command. [Search bar](https://manual.raycast.com/search-bar), [file search](https://manual.raycast.com/file-search)

An ordinary extension contributes command metadata to discovery, including optional search keywords. The command lifecycle begins when the user invokes it, including via an alias or fallback action. A command can then render a List and handle its own search-text changes, with throttling available for asynchronous/network queries. Discovering an extension's command and searching that service's remote records are distinct operations. [Manifest command properties](https://developers.raycast.com/information/manifest#command-properties), [command lifecycle](https://developers.raycast.com/information/lifecycle), [List API](https://developers.raycast.com/api-reference/user-interface/list)

Aliases can select a command quickly and, for argument-taking commands, focus its argument input. Fallback commands can receive unmatched root-search text when selected. Neither mechanism alone establishes that every installed integration is queried remotely on each root-search keystroke. [Aliases](https://manual.raycast.com/command-aliases-and-hotkeys), [fallback settings](https://manual.raycast.com/settings)

Conclusion from the reviewed documentation: Raycast combines host-integrated root results with command-scoped extension workflows. The docs do not justify describing root search as an automatic broadcast to every third-party service. Likewise, lack of a general third-party root-provider interface in the reviewed pages is not proof that every current Raycast feature uses the same internal path.

## Core boundary subsequently accepted

The core owns the launcher window/search input, command discovery, query routing, shared matching/ranking rules, result aggregation, keyboard navigation and action dispatch. Extensions supply features and their results: apps, file search, calculator, quicklinks and optional online integrations. Shipping these as default extensions does not prevent their results from appearing directly in the main search.

The core reads command/contribution metadata without running every extension. An enabled provider may be activated for matching queries/events; host aggregation should not wait for the slowest provider, and obsolete query results must be discarded. These are implementation recommendations consistent with accepted lazy activation, not a finalized SDK.

Q17 settled useful default local results and command discovery, with online service-content searches scoped by command/alias initially. Automatic searches across online integrations are deferred; additional global-provider participation can be designed later. A provider can also search a locally cached remote index; network origin alone does not dictate its performance or activation policy.

For Tinycast's implementation, see [the pinned source comparison](tinycast-root-search.md). The two reference products do not themselves settle our core/extension split.
