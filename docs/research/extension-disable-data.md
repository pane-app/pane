# Disabling extensions, uninstalling, and deleting data

Researched 2026-09-27 from official product and developer documentation. This is a documentation comparison, not a runtime audit. Recommendations below are proposals; the user has confirmed that default features must be disableable but has not chosen data-deletion semantics.

## What established products document

**Raycast separates clipboard controls.** Its manual provides a feature enable/disable toggle, configurable history retention, application exclusions, deletion of individual entries, bulk deletion by time window, and deletion of all entries. These are distinct controls. The page does not specify whether toggling the feature off erases existing history, how expiry runs while disabled, or whether enabling captures clipboard contents copied while it was off. We should not fill those gaps with assumptions. [Clipboard History](https://manual.raycast.com/clipboard-history)

**Raycast distinguishes background execution from command availability.** Background refresh has a separate preference; Store commands initially have refresh off, activated by first opening the command or changing its preference. Scheduling has execution timeouts. This is useful precedent for explicit control of work that continues while the launcher is hidden. It is not a blanket guarantee about the effects of disabling every kind of command. [Background Refresh](https://developers.raycast.com/information/lifecycle/background-refresh)

**Raycast distinguishes persistent storage and cache.** Its storage API uses an extension-scoped encrypted database and supports clearing an extension's entries. Larger files can live in the extension support directory. Its disk cache has capacity-based LRU eviction and is shared across an extension's commands by default. These pages do not establish a comprehensive uninstall cleanup contract; the manual documents uninstall as an action without specifying which stores survive it. [Storage](https://developers.raycast.com/api-reference/storage), [Environment](https://developers.raycast.com/api-reference/environment), [Cache](https://developers.raycast.com/api-reference/cache), [Extensions](https://manual.raycast.com/extensions)

**VS Code treats disable as a reversible installation state.** Extensions can be disabled globally or per workspace and remain disabled until enabled again. Uninstall is a separate operation; its current management documentation describes restarting the extension host for these changes. That documentation does not promise that uninstall erases all extension-created data. [Extension management](https://code.visualstudio.com/docs/configure/extensions/extension-marketplace#_manage-extensions)

**Chrome explicitly separates temporary and durable data.** Extension session storage is cleared on disable, reload, update, or browser restart. Extension local storage is cleared when the extension is removed. Thus loss of temporary runtime state is different from deletion of saved settings. Mozilla also documents clearing `storage.local` on uninstall, with Firefox developer preferences that can override that for testing. This is specifically about that API's local store, not every remote record or external file an integration may have created. [Chrome storage](https://developer.chrome.com/docs/extensions/reference/api/storage), [Mozilla storage.local](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/storage/local)

**Obsidian makes built-in features toggleable too.** Its settings distinguish core plugins and community plugins; both have enable/disable controls, while community plugins also have uninstall controls. This fits our small-core product direction, but those settings docs alone do not establish data-retention behavior. [Obsidian settings](https://obsidian.md/help/settings)

## Proposed behavior for our launcher

Do not make users choose a technical data policy before trying the app. Give each action a specific outcome:

| Action | Proposed effect |
| --- | --- |
| Disable | Stop extension execution and background work, remove its active commands/hotkeys/providers, discard temporary state; preserve settings, saved data, and stored credentials for re-enabling. Existing retention policies still apply. |
| Enable | Restore contributions on demand; reuse saved data; never resume an obsolete in-flight action. |
| Clear cache | Delete rebuildable host-managed cache only; retain settings, saved content, and credentials. |
| Delete extension data | Stop the extension first, explicitly confirm the affected host-managed settings/content/cache/credentials, then erase them; leave it disabled until the user enables it. |
| Uninstall | Stop execution and remove installed code/cache. Show an explicit choice to retain saved data for reinstall or delete the extension's local data too; default to retention to avoid accidental loss. Keep retained data visible in Storage settings. |

These are design choices, not claims that every compared application follows them. Destructive actions should state their scope. Disabling should be immediate and reversible without repeated confirmation. Bundled extensions may only be logically removable from a release bundle, but must incur no ongoing extension execution when disabled.

The host should own separate extension namespaces for settings, durable data, cache, and credentials. It should own cleanup of registrations and cancellation, and reject callbacks from unloaded generations. A broken extension must not need to run successfully before it can be disabled or its managed data deleted. With unrestricted native/Node code, detached processes or arbitrary external files cannot be covered by the same guarantee; that limitation is another reason the runtime/access decision matters.

Raycast explicitly allows filesystem access outside its storage APIs. Consequently, erasing an application's managed extension storage cannot be assumed to remove arbitrary files or reverse changes made through third-party APIs. Our deletion UI should distinguish local managed data from exported files, remote accounts, and provider token revocation. [Raycast security](https://developers.raycast.com/information/security)

## Clipboard-specific proposal

Clipboard history stays opt-in. Disable means unsubscribe immediately, cancel queued capture, and discard late capture results. Preserve unexpired history with clear wording: "Recording stopped. Saved history remains until it expires or you delete it." Provide a separate **Delete history** action and an optional **Pause recording** control that permits searching existing history.

Use a visible finite retention default, initially seven days as a product proposal rather than a research-derived requirement. Enforce expiry through host-managed expiry metadata and cleanup without activating the extension; purge overdue records before display after startup. Do not backfill the current clipboard when re-enabled. Include images, OCR text, previews, and indexes in history deletion. Any pinned-item exemption must be explicit; **Delete all history** should include pins.

OS clipboard contents, another clipboard manager's history, remote accounts, exports, and backups are separate data locations. Never describe deleting our history as clearing all of those. Local file deletion also is not a promise of forensic secure erasure. Application exclusion and sensitive-content hints need per-OS validation before promising consistent prevention of capture.

## Remaining unknowns

Raycast's exact clipboard-disable and uninstall persistence behavior needs a direct version-specific runtime test or an upstream answer if copying it precisely becomes necessary. We have sufficient evidence to design our own explicit contract now. The actionable distinction is between stopping a feature, removing its code, deleting its managed data, and changing external data; those should not be hidden behind one ambiguous toggle.
