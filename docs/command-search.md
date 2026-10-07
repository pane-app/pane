# Searching an online service inside its command

Added for [#30](https://github.com/hoangvu12/pane/issues/30): US11, US39,
US40; T03, T09; contributions to G2 and G3, not claims that they pass. An
extension command that searches an online service gets a **search field of
its own** once the user opens it; Pane sends it the text typed there and
lists what it finds. [Root search](root-search.md) never asks it: text typed
in root search reaches no such command, and so no service. Web requests go
through `wasi:http@0.3.0`, which Pane hosts for every command
([ADR 0018](adr/0018-extensions-reach-the-network-through-wasi-http.md),
proposed). The same on every system.

## What the user sees

Root search lists the command by its title like any other. Enter opens it:
the query field stays on screen, empty and focused (placeholder "Search"),
above the command's own list (the items of its tree, see
[list-tree.md](list-tree.md)). Typing sends the text,
trimmed, to the command; while it answers the status says "Running…" and
the rows listed stay; its results then replace the rows, the first
selected. Enter on a result runs the command's action for it, whose answer
shows in the status line, as for any item. Clearing the field shows the
command's own list as it was last listed, then asks the command for it
again, so it shows what changed since (a setting it displays, say); Escape
clears the text without asking, then leaves the command. Nothing found: "No
results for “…”". Forms of the command's own items open and return as
usual.

## Stopping searches and late answers

The runtime waits 150 ms before it starts a search. Each change of the text
starts a new search and stops the one before, in the runtime as well as in
the launcher:

- A search still queued behind other calls, or within its 150 ms, is never
  started: typing on asks only for the text the user stops at.
- A search waiting inside the guest (on its web request) is stopped there:
  the guest's call and its instance are dropped, as when a
  [generation](generations.md) ends, so the connection closes (the fixture
  service sees its client hang up) and nothing after the guest's `await`
  runs. The next search starts a new instance: in-memory state is lost.
  Wasmtime cannot cancel one call and keep its instance (the dropped call's
  task would resume on the next call), which is why the wait comes first.
- An answer that arrives anyway, or an older search's error, is never
  shown: the launcher's search epoch discards it.

Leaving the command (Escape, root search, another screen) stops its search
the same way. Disabling, reloading, updating, uninstalling or pausing the
package stops it through its generation, as for any call: its end changes
nothing on screen. A crash of the extension runtime ends it with the error
of a call the crash lost, in place of results.

## Web requests

Pane sends a command's requests itself (hyper, HTTP/1.1; TLS with rustls,
trusting the system's certificates) and bounds each, whatever the command
asks for: 10 s to connect, 20 s for the response's head, 10 s between two
pieces of its body, 30 s for the whole request, a body of at most 4 MiB, and
four connections open at once per package. Past a limit the request ends
with a `wasi:http` error the command receives ("the service did not answer
in time", "the service took too long to answer", "the answer is larger than
the 4194304 bytes Pane accepts"); a certificate the system does not trust is
"the host's certificate is not trusted". Redirects are not followed: a `3xx`
is the command's response. Any `http` or `https` address is allowed,
including this computer's own services, the local network and link-local
addresses such as `169.254.169.254`: extensions are trusted code.

Manage extensions says "Uses the network" on a package whose component
imports `wasi:http` (found when it is installed, updated or reloaded), and
lists a row per such package, **Network use of …**, whose details list the
addresses (`host:port`) it tried to reach since Pane started.

## Errors

An error the command answers with (the service unreachable, a status other
than 200, an answer it cannot read, a limit reached) is shown as the status
error in place of results, rows cleared: "The extension reported an error:
Could not reach the service at http://127.0.0.1:8740: connection refused",
or "…: The service answered 503: the registry is down for maintenance". It
is an ordinary operation error, so it never counts towards
[pausing](pausing.md) the extension, however often it happens; a stopped
search is not a failure either. A trap while searching is a crash, as for
any call.

## For extension authors

In `pane.json`, a command sets `"search": true`; its component then also
exports `pane:extension/command-search` ([wit/search.wit](../wit/search.wit)):
`search(command, query) -> result<list<search-result>, string>`, where a
result has an `id` (passed to `handle-event` as the callback id when
activated, which the SDKs hand to the command's `run_search_result` /
`runSearchResult`), a `title` and
an optional `subtitle`, the fields of a root result without its action,
and an optional `file` (#150): the id of a file of the folder granted to
the command's package, as `list-folder` gave it, when the result is that
file. Pane then lists it with the file's own name and folder and gives it
its own [file actions](files.md#the-file-actions) (Open, Reveal, Open
With…, Copy Path, Copy File, Move to Recycle Bin; for a program, Enter
reveals it and only Run runs it), which Pane performs without calling the
command; an id Pane did not give is not listed. A search answered while
that folder was still being listed is asked again once it is, unless a
newer text stopped it. Installing checks the export, as for the other
optional exports. A command may set both `"search"` and `"rootResults"`
(Search Files does): root search then asks it as well, so it declares that
what is typed there reaches it; a command that searches without saying
`rootResults` is still never asked by root search. Rust:
`pane_guest::search::Guest` and `pane_guest::search::export!`; JS/TS:
export `commandSearch` with `"pane": { "search": true }` in `package.json`.

Web requests: Rust `pane_guest::http::get(url, headers)`, JS/TS
`get(url, headers)` from `@pane/extension/http`, each returning the status,
headers and whole body; errors are readable ("connection refused"). Both
are thin wrappers over the standard `wasi:http@0.3.0` client
(`wasi:http/client.send`), whose bindings are available for anything else
(other methods, bodies, streaming). A JS/TS command imports `wasi:http`
only if its bundle uses it (itself or through `@pane/extension/http`).
Libraries that build on `wasi:http` or on these helpers work; ones that open
sockets themselves or need Node.js's or a browser's `fetch` do not. The
samples read JSON with `serde_json` (`no_std` + `alloc`) in Rust and
`JSON.parse` in JS/TS. See
[guests/README.md](../guests/README.md#searching-inside-a-command).

The samples, **Package search** (`guests/sample-search`, `-js`, `-ts`,
packaged in `guests/packages/sample-search*`), search the **fixture
service**: a made-up package registry on 127.0.0.1
([crates/pane-core/tests/support/service.rs](../crates/pane-core/tests/support/service.rs);
`cargo run -p pane-core --example fixture_service`, port 8740, the
samples' default address, or `--port 0` for a free one). A form item
changes the address (kept in the extension's settings). `GET /search?q=`
lists packages whose name contains the text; a text starting with `slow` is
held for ten seconds; `down` answers 503; `GET /packages/<name>` gives the
details Enter shows. Texts and names starting with `huge`, `stall` or
`drip` misbehave (an endless body, a head then nothing, a byte every
50 ms), and `misbehaving` lists packages whose details do.

## Checks

- `crates/pane-core/tests/command_search.rs`, for the Rust, JavaScript and
  TypeScript samples against the fixture service on a free port of
  127.0.0.1: typing in root search (the command's title, a package name,
  `slow`, `down`) sends the service nothing, while the same text in the
  command does; results, details, an encoded text, nothing found and a blank
  text; a cleared search lists the command as it is now; the first
  keystroke's search waits before asking anything, and keystrokes typed on
  within its wait ask only for the last text (the wait is held by the test
  through the debug builds' hidden `Runtime::set_search_timer`, not the
  clock, so no gap between keystrokes is assumed); a newer search stops the one the service
  holds (the service sees the hang-up well before its ten seconds) and the
  older answer never replaces the newer; an answer to an older search
  arriving after the newer one is not shown; Escape and leaving the command
  stop a search, and so do disabling, reloading and uninstalling the
  package, without changing the screen; a runtime crash ends a search with
  the lost call's error and the restarted runtime searches; an unreachable
  address (a port held bound, never listening) four times, then the
  service's 503, are errors and the extension keeps working; an endless
  body, a stalled head and a dripping body, for searches and for an action,
  end with the matching errors within short test limits (only the limit a
  case checks is shortened, so a slow machine cannot make another fire
  first); an untrusted
  certificate is "not trusted"; Manage extensions says which package uses
  the network and lists the address it reached; a manifest saying
  `"search": true` for a component without the export, or
  `"rootResults": true` too for one without that export, is refused at
  install. Search Files' results naming files are `file_actions.rs`'s.
- `crates/pane-core/src/http.rs`'s unit tests: code whose generation ended
  sends nothing, and a package's connections are capped.
- `crates/pane/tests/command_search.rs`: the same through the window with
  real key events: the query field becomes the command's, the results are
  rendered, Escape clears then leaves.
- The native smokes' search phase (screenshots 160 to 169; Linux run, see
  [Linux](platforms/linux.md#searching-inside-a-command-30)), against the
  fixture service's log, on a free port set through the sample's form.

No test or smoke reaches beyond 127.0.0.1.

## Limits

- **Provisional, pending user confirmation:** the manifest key
  `"search": true` and interface name `command-search`; results are plain
  rows whose action is `handle-event` with the result's id (no forms,
  custom views, platforms or
  Pane-performed actions such as opening a link); the 150 ms wait, and each
  stop of a started search dropping the instance; rows stay listed while a
  search runs; errors clear the rows; the error text keeps the "The
  extension reported an error:" prefix; the request limits above.
- While one extension's search waits for its service, or its 150 ms,
  other extensions' calls are served (#136); the command's own next calls
  (its next search, an action) wait for it, one call into an instance at a
  time. Only the outermost call watches the stop: an operation a searching
  guest waits for runs to its end first.
- Network access is not gated: extensions are trusted code (ADR 0002), and
  any extension may use `wasi:http`, from any call, including root-results
  providers. Only this command kind is kept out of root search. The
  addresses reached are kept for the session only; a package installed
  before Pane recorded network use says so once reloaded or updated.
- HTTP/1.1 only; no proxy settings; no redirects followed. Wasmtime's
  `wasi:http` 0.3 is marked experimental upstream.
- `wasi:http` types are not declared in TypeScript (`wasi.d.ts`); use
  `@pane/extension/http` or type them yourself.
- HTTPS against a real service, and the macOS and Windows smokes, have not
  been run.
