# Native fork integration smoke

2026-10-02, Windows 11 Pro 25H2 build 26200.8737, 96 DPI (100% scaling).
Normal Pane binary built using `cargo build --locked -j1 -p pane` from application
commit `fece16533d87ce77d84eca038bacf2309a018956`, with GPUI fork
`2b9e644e3f89a38eacebc713fb0d1807c618c76c`.

Binary SHA-256: `BEC481F869FAC4322A588ADD046CB721F0BD7156C02BF6E8627A60E0618BEC20`.
Capture helper: `scripts/capture-pane-windows.ps1` from parent commit
`0980135364792ffb993cc156c1fca5413fa18aa2`; file SHA-256
`1F2B8D3D71DEFBC3C1F9FCF751FDDF29DA2D1013E84029561FB2BFA4A19D7B58`.

Reproduction with the helper in the integrated checkout:

```powershell
./scripts/capture-pane-windows.ps1 `
  -Binary "$PWD/target/debug/pane.exe" `
  -OutputDir "$PWD/target/native-ui63" `
  -ExtensionsDir "$PWD/target/guests" `
  -ApplicationRevision fece16533d87ce77d84eca038bacf2309a018956 `
  -PositionTopLeft -ClickToFocus -Keys @('rust', '{ESC}')
```

The desktop slot was coordinated with the parent before launch. The helper moved
only its spawned window to the top-left so retained prototype windows did not
occlude it. Initial foreground activation failed; guarded-click activation
verified that the click hit the spawned window, then confirmed its exact HWND
before each key token. It restored the pointer. Data and LOCALAPPDATA/cache were
isolated to this run; no default downloads or global settings changes occurred.
Only spawned PID 66844 was closed, and its exit was confirmed. Stderr was empty.
The original prototype processes were not modified or stopped.

Visual inspection of the actual window captures confirms:

- `00-initial-window.png`: root search, editable field and real sample results.
- `01-after-rust-window.png`: the query reads `rust`, with only Rust sample shown.
- `02-after-ESC-window.png`: the query is cleared and root results return.

`pane-run.json` retains source revision, binary hash, Windows version, DPI, focus
checks, capture times, isolation paths and cleanup results. Absolute paths are
historical evidence, not dependency configuration. Its transparency setting was
1, but this unchanged launcher uses its existing opaque appearance: no styled
UI, native glass, compositor-blur, IME or screen-reader claim is made here.
