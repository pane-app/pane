param(
    [Parameter(Mandatory=$true)][string]$Wasmtime,
    [Parameter(Mandatory=$true)][string]$WasmTools,
    [string]$TargetDirectory = (Join-Path ([IO.Path]::GetTempPath()) 'kyoko-wasi03-rust-target')
)
$ErrorActionPreference = 'Stop'
$manifest = Join-Path $PSScriptRoot 'Cargo.toml'
& cargo build --release --locked --target wasm32-wasip2 --manifest-path $manifest --target-dir $TargetDirectory
if ($LASTEXITCODE -ne 0) { throw 'Rust guest build failed.' }
$component = Join-Path $TargetDirectory 'wasm32-wasip2/release/wasi03_rust_spike.wasm'
& $WasmTools validate --features all $component
if ($LASTEXITCODE -ne 0) { throw 'Component validation failed.' }
$wit = @(& $WasmTools component wit $component)
if ($LASTEXITCODE -ne 0) { throw 'Component inspection failed.' }
$imports = @($wit | Where-Object { $_ -match '^  import ' })
if ($imports.Count -eq 0) { throw 'No component imports found.' }
if ($imports | Where-Object { $_ -notmatch '@0\.3\.0;' }) { throw 'Found an import outside WASI 0.3.0.' }
$wit | Set-Content (Join-Path $PSScriptRoot 'component.wit')
$view = @(& $Wasmtime run -S p3 --dir $PSScriptRoot $component)
if ($LASTEXITCODE -ne 0) { throw 'WASI 0.3 execution failed.' }
$parsed = ($view -join "`n") | ConvertFrom-Json
if ($parsed.items[0].id -ne 'rust-p3') { throw 'Unexpected guest output.' }
[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'view.json'), ($view -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
& $Wasmtime run -S p3 $component
$deniedExit = $LASTEXITCODE
if ($deniedExit -ne 1) { throw "Expected denied execution exit 1, received $deniedExit." }
$result = [ordered]@{
    rustc = (& rustc --version)
    wasmtime = (& $Wasmtime --version)
    wasm_tools = (& $WasmTools --version)
    component_bytes = (Get-Item $component).Length
    component_sha256 = (Get-FileHash $component -Algorithm SHA256).Hash.ToLowerInvariant()
    imports = $imports
    success_exit = 0
    no_preopened_directory_exit = $deniedExit
}
$result | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $PSScriptRoot 'result.json')
$view
$result | ConvertTo-Json -Depth 5
