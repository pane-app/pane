# Pi reload behavior (Q14)

Researched 2026-09-27. GitHub's current `main` resolved to `2b0a123de98318c2ff8069661721ce0c3794c34e`; source links below pin that commit. This covers the established coding-agent extension runtime, not the separate Chord runtime. Source review only; no downloaded extension code was executed.

## Trigger and scope

Interactive Pi has an explicit `/reload` command, and extension command handlers can call `ctx.reload()`. The ordinary operation is a manual reload of the extension runtime and resources, not replacement of only the file that changed. The inspected path does not provide an automatic extension source watcher. Pi remains in the same application process.

Interactive reload refuses to proceed during a streaming response or compaction. It reloads extensions, skills, prompts, themes, context files and keybindings. This makes it broader than a command-specific developer refresh. [Interactive implementation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L6177), [command context reload](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/runner.ts#L921)

## Lifecycle

`AgentSession.reload()` captures flag values, emits `session_shutdown` with reason `reload`, invalidates the old runner, refreshes settings/providers/resources, constructs a new extension runtime, then emits `session_start` with reason `reload` when runtime bindings exist. It retains the session object and passes the current active-tool names and prior flag values into reconstruction. [Session reload](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/agent-session.ts#L3291)

Resource reload clears the extension factory cache. Imports use `jiti` with `moduleCache: false`, and extension factories register contributions anew. Invalidating the old runtime rejects guarded stale API operations and unsubscribes tracked event-bus listeners. This does not forcibly terminate every timer, process, socket or arbitrary JavaScript closure created by trusted extension code. [Resource reload](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/resource-loader.ts#L444), [factory cache and invalidation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/loader.ts#L134), [module import](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/loader.ts#L479)

## State and UI

The conversation survives: the interactive frontend rebuilds the chat from the existing messages. Extension UI is explicitly reset: overlays/dialogs, widgets, status entries, custom header/footer/editor and extension input listeners are removed or reset. The editor replacement helper transfers text, but there is no general contract preserving arbitrary custom UI object state. [Reload frontend](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L6177), [UI reset](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L2352), [editor text transfer](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L2764)

Extension authors must reconstruct their own state. Pi documents tool-result details for branch-sensitive tool state, `pi.appendEntry()` for durable session data excluded from model context, and external storage for data outside a session. `session_start` is where branch-sensitive state should be reconstructed. Authors are told to start long-lived resources during session start or use, clean them up idempotently during `session_shutdown`, and not reuse old contexts after reload. This is explicit save/reconstruct behavior, not automatic migration of live JavaScript variables. [Extension lifecycle and state documentation](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/docs/extensions.md)

## Failure behavior

The loader catches individual import/factory errors, records diagnostics, skips the failed extension, and continues loading other entries. A failed factory discards staged registrations. The interactive frontend reports broader reload failures and removes its temporary reload display. [Extension loading](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/loader.ts#L535), [interactive error handling](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/modes/interactive/interactive-mode.ts#L6260)

There is no general last-known-good extension rollback in this flow: shutdown and invalidation of the old runtime happen before replacement imports finish. That conclusion follows from the operation order; it is not a claim that every failure terminates Pi. Cleanup handlers also cannot reverse arbitrary external side effects.

## Implication for this launcher

Pi supports keeping the host and durable session data while reconstructing extension code/UI. It does **not** establish that only the changed extension restarts, or that arbitrary active tasks survive reload. The user subsequently accepted per-extension development reload, automatic rebuild and keeping working code after a failed build in [ADR 0004](../adr/0004-reload-extensions-without-restarting-launcher.md). These are launcher decisions, not claims about Pi's behavior.
