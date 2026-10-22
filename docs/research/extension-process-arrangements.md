# Extension process arrangements: Raycast and Pi

Researched 2026-09-27 for Q23. Source/documentation review only; no application or resource benchmark. The user subsequently accepted the shared Node helper/worker baseline in Q23; see [ADR 0009](../adr/0009-shared-node-helper-with-extension-workers.md). Q24 subsequently chose best-effort API stability rather than indefinite compatibility; see [ADR 0010](../adr/0010-best-effort-extension-api-compatibility.md).

## Raycast's documented architecture

Raycast documents a native application process and a separate managed Node child process. Loaded extensions run in workers within that Node process, each with its own JavaScript execution environment and event loop; communication crosses back to the application through a host protocol. A worker is not another OS process. [Runtime model](https://developers.raycast.com/information/security)

The engineering explanation describes creating/destroying workers, heap limits and error screens after worker failure. It also explains that a Node-process crash usually leaves the native app alive, while native host bugs can still crash the app. Its concern about the cost of a process per extension is design rationale, not our measurement. The article describes historical architecture; it is not a fresh inspection of every current Raycast platform/build. [Engineering article](https://www.raycast.com/blog/how-raycast-api-extensions-work)

Node's workers are threads, with differences from main-thread APIs and conditions for native add-ons. Their separation does not guarantee survival of the entire Node process under native crashes or process-wide failures. A process failure loses all workers in that process. [Node workers](https://nodejs.org/api/worker_threads.html#class-worker)

## Conventional Pi

Pi imports extensions into the coding-agent process and invokes their factories and registered handlers directly. It does not create a host-owned worker or process per conventional extension. Extension authors can independently use subprocesses/workers/native tools; that is different from automatic host separation. Pi's npm distribution uses Node; its inspected standalone build uses Bun. See [targeted Pi source check](pi-process-arrangement.md), [runtime audit](pi-tinycast-node-compatibility.md).

Enabled extension factories load on resource initialization, while command/tool handlers execute when used. Heavy work can be deferred by authors, but host loading is not our accepted manifest-driven lazy activation. Conventional `/reload` rebuilds extension/resources in the same application process; it does not replace one extension worker. [Activation](pi-activation-setup.md), [reload](pi-reload.md)

Pi catches many ordinary load/handler errors, but a last-resort uncaught exception can terminate the application. Synchronous work in an extension handler also occupies its calling thread; a slow or looping callback has no automatic per-extension event-loop separation. These are consequences of the inspected execution arrangement, not failure-injection measurements. [Error handling source audit](pi-failures-updates-offline.md)

## Comparison and launcher recommendation

| Question | Raycast documented arrangement | Conventional Pi inspected arrangement |
|---|---|---|
| Where does extension JS execute? | Separate Node child process | Coding-agent process itself |
| Host-created boundary between extensions | Separate workers | Ordinary imports/calls, no worker per extension |
| Host survives JS runtime process termination? | Native app is a separate process; not an absolute guarantee | Runtime termination terminates Pi |
| Reload boundary | Workers provide replaceable execution contexts; exact current reload internals not established | Extension/resource reload within the same process |
| Resource winner established here? | No | No |

Accepted initial arrangement: a Raycast-style shared Node helper with a worker per active JS extension as the initial prototype baseline. Keep GPUI CE in the launcher process, and retain native helper processes for Rust. This preserves the previously chosen Pi-inspired authoring/trust/distribution model without copying its in-process terminal runtime arrangement.

Measure full process-tree memory, startup, event latency and cleanup before claiming an overhead benefit. Verify ordinary and native npm dependencies in workers; evaluate a separate-process path where compatibility requires it. Installed-but-unused extensions remain inactive under Q15. Do not interpret a worker as a security sandbox, unlimited compatibility, or guaranteed separation of fatal native crashes. The shared-worker baseline is accepted subject to prototype checks. A separate-process exception remains an uncommitted candidate if compatibility requires it.
