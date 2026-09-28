# Operations

Added for [#22](https://github.com/hoangvu12/pane/issues/22) (US46–US49, T14,
G2, G4), following [ADR 0011](adr/0011-extension-call-and-result-api.md). An
**operation** is a named, versioned function an installed package publishes
for other extensions to call through Pane. This slice is the minimum for one
extension to reuse another across Rust, JavaScript and TypeScript: one call,
one JSON input, one JSON result or an explained error. It is not a workflow
engine, and dependency declarations and installing missing targets are
[#42](https://github.com/hoangvu12/pane/issues/42).

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
- A package may publish operations and no commands; it then adds nothing to
  root search. `commands` may be omitted when `operations` is not empty.
- The package preview lists them ("Operations: greet (version 1)"). Their
  components are checked and copied at install like commands'.

The component named there serves calls through
`run-operation(operation, input) -> result<string, string>` in the
`published-operations` interface of
[`wit/operations.wit`](../wit/operations.wit), which it exports beside
`command`, like a command computing [root results](root-search.md) exports
`root-results`: in Rust it implements `pane_guest::publish::Guest` and calls
`pane_guest::publish::export!`; a JS/TS package sets
`"pane": { "operations": true }` in its `package.json` and its module exports
`publishedOperations`. Pane calls it only for an operation the manifest
publishes, naming it. Installing checks the export, without running guest
code, for every component that serves an operation; a component that
publishes none does not export it and is unchanged.

### Calling

A guest calls with `pane:extension/operations.call` in
[`wit/operations.wit`](../wit/operations.wit), imported by the world Pane
hosts (`extension-with-settings`):

```wit
call: async func(source: string, operation: string, version: u32, input: string)
  -> result<string, call-error>;
```

- **Addressing by identity.** `source` names the target package by its
  [package identity](../CONTEXT.md), never its title: `local:` and its
  source folder's path, absolute (as Pane shows it) or relative to the calling
  package's own source folder (`local:../sample-operations-js`), resolved like
  the identity at install time. Another scheme is `not-found` until Pane
  installs from npm or Git. A command built into Pane has no source folder, so
  only absolute sources work from it.
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
  | `unavailable` | The package does not support this system, or the runtime stopped. |
  | `failed` | The operation ran and returned an error. |
  | `crashed` | The target trapped. Its instance is dropped; the next call starts it afresh. |
  | `refused` | The call would reach a package already serving a call in the same chain, the chain is too deep, or the input or result is not JSON within the limit. |

  The caller always gets an answer and keeps working; what it shows is up to
  it. The samples show `<kind>: <message>`, such as "failed: a name is
  needed".

In Rust, `pane_guest::operations::call(source, operation, version, input)`
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
- **Bounded chains.** A package's component is busy from the start of its
  call until it returns, so a call that would reach one already in the chain
  (a package calling itself, or A calling B calling A) is `refused` at once
  instead of waiting on itself. A chain holds at most 8 calls
  (`MAX_CALL_DEPTH`), counting the caller's own; the ninth is refused. An
  intermediate operation receives the refusal as its call's error and decides
  what to return (the fixture reports it as its own `failed` error).
- **Cancellation ownership.** A call belongs to the guest call that made it
  and never outlives it: the caller's call cannot finish while its operation
  runs. If the caller abandons the call (drops the future) before Pane starts
  it, it is not started; one already running finishes and its answer is
  discarded. Pane has no timeouts or user cancellation yet (#14), so a target
  that never returns holds the caller and, as with any hung guest call, the
  runtime. A target disabled while serving a call finishes, and the caller
  gets `disabled` instead of its answer.
- **Concurrent calls from one caller** are served one after another. One
  that arrives while another operation of the same chain is running is served
  inside that chain, so if it targets a package already in the chain it is
  refused like a cycle.

## Examples and tests

- Samples, each publishing `greet` version 1 and calling another's:
  [Rust](../guests/sample-operations/src/lib.rs) ("Call from Rust" asks the
  JavaScript and TypeScript samples),
  [JavaScript](../guests/sample-operations-js/src/index.js) and
  [TypeScript](../guests/sample-operations-ts/src/index.ts) (both ask Rust).
  Installed side by side from `target/guests/packages/`, they answer "Rust
  answered: Hello, JavaScript, from Rust" and so on, show the target's own
  error ("failed: a name is needed") and a missing package (`not-found`).
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
- No declared dependencies: a caller names its targets in code, so Pane
  cannot show, install or check them before a call (#42), and disabling a
  target does not consider its callers (#43).
- No timeouts or cancellation of a running operation (#14).
- A target whose update or disable happens while it serves a call is
  handled like a command's call in the same situation (#10, #11).
