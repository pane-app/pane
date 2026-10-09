# Operations

Added for [#22](https://github.com/pane-app/pane/issues/22) (US46–US49, T14,
G2, G4), following [ADR 0011](adr/0011-extension-call-and-result-api.md). An
**operation** is a named, versioned function an installed package publishes
for other extensions to call through Pane. This slice is the minimum for one
extension to reuse another across Rust, JavaScript and TypeScript: one call,
one JSON input, one JSON result or an explained error. It is not a workflow
engine; [dependency declarations and installing missing targets](dependencies.md)
came with [#42](https://github.com/pane-app/pane/issues/42).

## Contract

### Publishing

A package publishes operations in its `pane.json`; nothing else is callable,
so a command never becomes a headless API by accident (US49):

```json
"operations": [
  { "id": "greet", "version": 1, "component": "sample_operations.wasm" }
]
```

- `id`: unique in the package. `version`: a positive integer, the version of
  the operation's input and result; a change that breaks callers publishes a
  new version. `component`: the component serving it, relative to the package
  folder, often a command's component too (then one instance serves both).
  `platforms` (optional, as for commands): the systems it works on;
  elsewhere a call to it is `unavailable`.
- A package may publish operations and no commands; it then adds nothing to
  root search. `commands` may be omitted when `operations` is not empty.
- The package preview lists them ("Operations: greet (version 1)"). Their
  components are checked and copied at install like commands'.

The component named there serves calls through
`run-operation(operation, input) -> result<string, string>` in the
`published-operations` interface of
[`wit/operations.wit`](../wit/operations.wit), which it exports beside
`command`, like a command computing [root results](root-search.md) exports
`root-results`: in Rust it implements `pane_extension::publish::Guest` and calls
`pane_extension::publish::export!`; a JS/TS package sets
`"pane": { "operations": true }` in its `package.json` and its module exports
`publishedOperations`. Pane calls it only for an operation the manifest
publishes, naming it. Installing checks the export, without running guest
code, for every component that serves an operation; a component that
publishes none does not export it and is unchanged.

### Calling

A guest calls with `pane:extension/operations.call` in
[`wit/operations.wit`](../wit/operations.wit), imported by the world Pane
hosts (`extension-with-data`):

```wit
call: async func(source: string, operation: string, version: u32, input: string)
  -> result<string, call-error>;
```

- **Addressing by identity.** `source` is the target package's
  [package identity](../CONTEXT.md) exactly as installed, never its title:
  `local:` and the absolute path of the folder it was installed from, as
  Pane resolved it (the path Settings › Extensions shows after "local folder";
  `PackageIdentity::key` on the host). A relative path, or a spelling of the same folder
  other than the resolved one, is not an identity and is `not-found`; so is
  another scheme until Pane installs from npm or Git. A caller learns its
  targets' identities from its user or configuration (the samples ask in a
  form), or declares them: since #42 `source` may instead be the id of a
  dependency the caller's `pane.json` declares (for the operations it declares there), which Pane resolves to the
  identity recorded when the caller was installed
  ([dependencies](dependencies.md#addressing)).
- **Input and result** are JSON text (any JSON value), at most 1 MiB each
  (`MAX_OPERATION_JSON`). Pane checks both before passing them on; their
  shape is the operation's documented contract at that version.
- **Errors** are a record of `kind` and `message`. `failed` is the
  operation's own error, its `message` exactly what the operation returned;
  every other kind is a lifecycle failure Pane explains:

  | Kind | When |
  | --- | --- |
  | `not-found` | No installed package has that source, the source is not `local:`, or the package does not publish that operation (the message lists what it does publish). |
  | `disabled` | The package is disabled. Pane does not enable it, start it, or ask. |
  | `incompatible` | It publishes the operation at another version, its installed copy cannot load, or Pane cannot run its component (WASI 0.2, older API shape, not a component). |
  | `unavailable` | The package or the operation does not support this system, or the runtime stopped. |
  | `failed` | The operation ran and returned an error. |
  | `crashed` | The target trapped. Its instance is dropped; the next call starts it afresh. |
  | `refused` | The call would reach a package already serving a call in the same chain, the chain is too deep, the input or result is not JSON within the limit, or the guest called while Pane was not running a call of it. |

  The caller always gets an answer and keeps working; what it shows is up to
  it. The samples show `<kind>: <message>`, such as "failed: a name is
  needed".

In Rust, `pane_extension::operations::call(source, operation, version, input)`
is an `async fn` returning `Result<String, CallError>`; `CallError::explain()`
gives `<kind>: <message>`. In JavaScript and TypeScript,
`call` from `"pane:extension/operations@0.1.0"` returns a `Promise<string>`
that rejects with an object whose `payload` is `{ kind, message }`
([`operations.d.ts`](../guests/js/operations.d.ts)).

## Behavior

- **Routing and lazy activation.** Every call goes through the host, which
  resolves the target among the installed packages as the launcher has them
  at that moment (an enable or disable applies at once). A target runs only
  when called: installing or listing starts nothing, and a call starts the
  target's instance only if it is not running. Instances are per component,
  so a target whose command is open shares that instance.
- **Data isolation.** The target runs with its own
  [extension settings](../guests/README.md#keeping-settings), owned by its
  identity; a caller never reads or writes them, and the reverse.
- **One thread, no deadlock.** The runtime serves all guest calls on one
  thread, one at a time. A guest that calls an operation is suspended inside
  its own call; the runtime takes the caller's instance aside and serves the
  operation on the same thread in the meantime (starting the target if
  needed), then resumes the caller with the answer. The operation may itself
  call further operations the same way.
- **Bounded chains.** A package is busy from the start of a call of any of
  its components until it returns, so a call that would reach a package
  already in the chain (a package calling itself, even an operation served by
  another of its components, or A calling B calling A) is `refused` at once
  instead of waiting on itself. A chain holds at most 8 calls
  (`MAX_CALL_DEPTH`), counting the caller's own; the ninth is refused. An
  intermediate operation receives the refusal as its call's error and decides
  what to return (the fixture reports it as its own `failed` error).
- **Cancellation ownership.** A call belongs to the guest call that made it
  and never outlives it: the caller's call cannot finish while its operation
  runs. If the caller abandons the call (drops the future) before Pane starts
  it, it is not started (tested). The caller's guest is not polled while its
  operation runs, so it cannot abandon a running call. A call also belongs
  to the target's [generation](generations.md) and, through the chain, to
  every caller's: a target disabled, reloaded or updated while it serves a
  call is stopped at once and the caller gets `disabled` or `unavailable`
  instead of its answer; a caller stopped meanwhile stops the operation it
  waits for, whose instance is dropped too (the target stays enabled and
  the next call starts it afresh). A target computing for 5 seconds
  without finishing is stopped as unresponsive (#18,
  [pausing](pausing.md#when-an-extension-stops-responding)) and its caller
  gets `crashed` ("b stopped responding: ..."); a target that waits forever
  (on a clock, say) has no time limit and holds the caller and, as with any
  waiting guest call, the runtime. There is no user cancellation yet.
- **Concurrent calls from one caller** (Rust `join!`, JavaScript
  `Promise.all`) are served one after another in the caller's own frame: each
  frame serves only its own guest's calls, so a second call is never taken
  for the operation serving the first, and is not refused as a cycle.
- **Calls outside a Pane call.** A guest can call only while Pane is running
  a call of it. A call made at another time, such as while its component
  starts or from work left running after its call returned, is `refused`
  ("operations can only be called while serving a Pane call"), as is one its
  call returned without waiting for. This is not tested with a guest: none
  of the fixtures runs code outside a call.

## Examples and tests

- Samples, each publishing `greet` version 1 and with a command whose form
  asks for another package's identity, a name, and whether to ask once or
  twice at once: [Rust](../guests/sample-operations/src/lib.rs),
  [JavaScript](../guests/sample-operations-js/src/index.js) and
  [TypeScript](../guests/sample-operations-ts/src/index.ts). They answer
  "Hello, Rust, from JavaScript" and so on, show the target's own error
  ("failed: a name is needed") and a missing package (`not-found`). They
  also publish `wait` version 1, which waits ten seconds, and a second item,
  "Wait in another extension", calls it: disabling or reloading either
  package meanwhile stops the call ([generations](generations.md)).
- [`guests/fixtures/operations`](../guests/fixtures/operations/src/lib.rs):
  a Rust fixture the tests install as several packages to drive every error
  kind, cycles, the depth limit and settings isolation.
- [`crates/pane-core/tests/operations.rs`](../crates/pane-core/tests/operations.rs)
  asserts all of it through the launcher's public interface, and the native
  smoke scripts install the Rust and JavaScript samples and show a
  cross-language answer in the real window.

## Limits

- Only `local:` sources; npm and Git identities come with those sources.
- One string of JSON in and out; no streams, resources or schemas Pane
  validates beyond JSON syntax. The version is a single integer that must
  match exactly; there is no range or negotiation.
- Declared [dependencies](dependencies.md) (#42) are shown, checked and
  installed with the caller, but disabling a target does not consider its
  callers yet (#43).
- No time limit on waiting: a running operation stops only when a
  generation in its chain ends, or when it computes for 5 seconds without
  finishing (#18, [generations](generations.md#what-stopping-cannot-do-yet)).
  An operation that stops responding answers `crashed`, as the WIT has no
  kind of its own for it (provisional).
