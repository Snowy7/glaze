# Run through child Windows PowerShell -File (cargo warnings must not become terminating errors).
param([string]$Target = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'wasm-guests' } else { Join-Path $PSScriptRoot '..\..\target\release-plugins' }))
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
& "$repo\scripts\build-plugins.ps1" -Target $Target
pnpm --dir "$repo\plugins\hello-ts" run build
if ($LASTEXITCODE -ne 0) { throw 'TypeScript plugin bundling failed.' }
# The SDK's generated WIT can lag the native host. Componentize against the canonical WIT,
# so an otherwise successful JS build cannot silently ship an obsolete UI variant layout.
pnpm --dir "$repo\packages\plugin-sdk-ts" exec node node_modules/@bytecodealliance/jco/dist/jco.js `
    componentize "$repo\plugins\hello-ts\.hello-ts-build\plugin.js" --wit "$repo\crates\plugin-api\wit" `
    --world-name plugin --backend quickjs --backend-qjs-disable-async --backend-qjs-stub-wasi `
    -o "$repo\plugins\hello-ts\hello-ts.wasm"
if ($LASTEXITCODE -ne 0) { throw 'TypeScript component build against the current host WIT failed.' }
