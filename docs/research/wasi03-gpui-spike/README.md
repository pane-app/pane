# GPUI CE structured-view probe

This is a bounded renderer experiment. It reads a language-neutral JSON result,
adapts it to native GPUI CE elements, reloads that file on demand, and writes row
activation events to a JSONL file. It does not load or execute Wasm components.

The renderer is pinned to GPUI CE commit
`17d9c8e8fdb30a329d817ca06bff424e8e848f1a` (core package version `0.2.2`).

## Interface

Input file:

```json
{"title":"Results","items":[{"id":"first","title":"First result","subtitle":"Optional description"}]}
```

Each click appends one event to the second command-line path:

```json
{"type":"activate","id":"first"}
```

Replace the input file, then click **Reload JSON** to replace the displayed model.
An invalid replacement preserves the last valid view and displays an error.
This tests replacing view data; it does not reload guest code or preserve guest state.

## Windows reproduction

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'kyoko-gpui-view-target'
cargo build --manifest-path docs/research/wasi03-gpui-spike/Cargo.toml --locked -j 4
$probeDir = (Resolve-Path docs/research/wasi03-gpui-spike).Path
& "$env:CARGO_TARGET_DIR/debug/kyoko-gpui-view-probe.exe" "$probeDir/view.json" "$env:TEMP/kyoko-gpui-events.jsonl"
```

The executable uses the Windows GUI subsystem so launching it does not create a
console window. Closing the probe window exits its process. Each row's callback
routes only an event into a file; it does not execute a command selected by JSON.
The synchronous file operations are deliberate spike shortcuts, unsuitable for
slow or untrusted storage on a production UI thread.

## Prerequisites observed on the test machine

- Rust `1.97.0`, host `x86_64-pc-windows-msvc`. Pinned GPUI CE requires Rust `1.95`.
- Visual Studio Build Tools 2022 `17.14.37411.7`, MSVC tools `14.44.35207`.
- Windows SDK libraries `10.0.26100.0`.
- CMake was already installed. No system package installation was performed.

The source's Windows backend uses Direct3D 11. A successful build does not alone
verify GPU initialization or rendered interaction; see the recorded result below.

## Result

Verified on Windows on 2026-09-28 (Asia/Bangkok): build succeeded, native rendering
succeeded, view replacement succeeded, and activation callbacks succeeded.
`cargo fmt --check` also passed. No missing system dependency was encountered.

The smoke run used the actual executed guest outputs from the sibling probes:

1. Loaded `../wasi03-js-spike/view.json`: the window showed the TypeScript title
   and Calculator row. A native Windows click message emitted `calculator`.
2. Replaced that input with `../wasi03-rust-spike/view.json`, clicked **Reload JSON**,
   and observed the Rust title, filesystem result and subtitle. Clicking that row
   emitted `rust-p3`.
3. Asserted the two event IDs, captured the rendered window, and closed the process.

Reproduce the native smoke after building and producing the sibling guest outputs:

```powershell
& docs/research/wasi03-gpui-spike/smoke.ps1 `
  -InitialView docs/research/wasi03-js-spike/view.json `
  -ReplacementView docs/research/wasi03-rust-spike/view.json
```

The smoke identifies the native window by process ID, reveals it for the test,
and sends `WM_MOUSEMOVE`/`WM_LBUTTONDOWN`/`WM_LBUTTONUP` at the probe controls.
This exercises GPUI click dispatch and the same handlers used by manual clicks.
Coordinates are specific to this small layout, with DPI adjustment; this is not
a general UI automation framework. The screenshots were visually inspected.

Evidence: [initial screenshot](evidence/initial.png),
[reloaded screenshot](evidence/reloaded.png),
[activation events](evidence/events.jsonl), [asserted result](evidence/result.json),
[initial guest payload](evidence/initial-view.json),
[replacement guest payload](evidence/replacement-view.json).

The TypeScript producer is a **mixed WASI component**: its clock import is 0.3,
but its QuickJS engine still imports 0.2 interfaces. It does not satisfy the
P3-only requirement. The Rust producer passed its separate P3-only guest probe.
This renderer result establishes that both produced payloads can drive the same
native view and event adapter. Guest execution and rendering were separate
process steps connected by a file; no embedded Wasmtime host, guest action callback,
code hot reload, arbitrary custom-view protocol or cross-platform parity was
implemented or established by this experiment.
