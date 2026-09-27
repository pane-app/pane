# Extension startup failure after reload or update

Research for Q31, 2026-09-27. The user asked how other systems handle new extension code that builds/installs successfully but fails when it starts. The user subsequently accepted Q31 with "ok go with that prob"; see [reload ADR](../adr/0004-reload-extensions-without-restarting-launcher.md). Read-only source and documentation review; no runtime crash experiment was performed.

## Separate the failure stages

A build failure before replacement, a package installation failure, a module-import/activation failure after replacement, and an entire runtime-process crash have different recovery paths. Restarting the same extension host does not mean reinstalling the previous extension version. Reverting code does not itself undo data migrations or external side effects. These distinctions frame the comparison; they are not claims that every product implements the same recovery policy.

## Pi

At commit `2b0a123de98318c2ff8069661721ce0c3794c34e`, `AgentSession.reload()` emits shutdown and invalidates the old runner before reloading resources and building the new runtime. The inspected flow does not retain and restore the previous runner when replacement code fails. This is a control-flow conclusion, scoped to conventional Pi reload. [Session reload](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/agent-session.ts#L3291)

The loader catches import/factory errors, records a failure and continues with other extensions. A factory that throws has its staged host registrations discarded. This is cleanup of staged registrations, not restoration of older extension code or reversal of arbitrary filesystem/network changes. [Loader](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/loader.ts#L536)

A `session_start` handler throwing after loading is a separate case: the runner's event dispatch catches and reports handler errors, then continues. It does not automatically uninstall/downgrade the extension or necessarily mark the whole extension disabled. Do not describe all startup errors as failed import/skipped extension. [Event dispatch](https://github.com/earendil-works/pi/blob/2b0a123de98318c2ff8069661721ce0c3794c34e/packages/coding-agent/src/core/extensions/runner.ts#L988)

## Raycast

Development mode automatically reloads commands on save. If rebuilding fails, the last successfully built command keeps running and build diagnostics are shown. That behavior occurs before successful replacement; it is not evidence of post-startup rollback. [CLI](https://developers.raycast.com/information/developer-tools/cli#development)

Unhandled exceptions and rejected promises produce an error overlay. Development shows stack traces; production shows an error message. The reviewed docs do not establish automatic restoration of the previous version after a runtime/startup error. Raycast's closed runtime was not audited, so the defensible statement is no documented guarantee, not proof that every internal path lacks rollback. [Debugging](https://developers.raycast.com/basics/debug-an-extension#unhandled-exceptions-and-promise-rejections)

## VS Code

The inspected activation path loads the selected extension module and calls `activate`. If that fails, the activator records a failed extension and reports/logs the error. It does not switch to the previous extension version in that failure path. A new request to the same activator reuses its cached activation operation. [Pinned activation failure handling](https://github.com/microsoft/vscode/blob/4336606aa949efdcddb89b3db3e216562730b2bf/src/vs/workbench/api/common/extHostExtensionActivator.ts#L385)

A fatal extension-host process crash has a separate bounded restart mechanism. The package installer also has installation-group rollback, distinct from a successful install whose extension later fails to activate. The manual documents **Install Another Version** as a user-controlled way to select an available earlier version. This does not reverse extension data changes. [VS Code audit with source anchors](vscode-activation-failure.md), [manual version selection](https://code.visualstudio.com/docs/configure/extensions/extension-marketplace#install-an-extension)

## Accepted launcher policy (Q31)

Keep the accepted Q14 behavior for build failure: retain working code and show diagnostics. After replacement code starts and startup fails, clean up its host-managed work, report the affected instance as failed, and offer Retry/logs; allow the normal developer save/build/reload cycle to recover after a fix. Do not automatically restore older code. This is the accepted launcher policy informed by the comparison, not exact Pi behavior for every event-handler exception.

Keep prior Q20 suppression of repeatedly failing background activation. Ordinary users can use existing disable/update controls; dedicated manual downgrade UX is a separate unselected feature. Preserve managed saved data rather than automatically deleting or reversing it, but do not promise that trusted extension code cannot corrupt it. Existing selected shared Node topology still means a fatal process crash can affect several extension workers. Exact cleanup and runtime-restart mechanics remain implementation work.
