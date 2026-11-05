# Builds the in-repo WebAssembly plugins (docs/plugins.md) into their plugin folders, where the
# shell finds them as bundled plugins; deploy-local.ps1 then AOT-compiles them.
#   powershell -NoProfile -File scripts/build-plugins.ps1 [-Target C:\mypaper-targets\wasm-guests]
param([string]$Target = "C:\mypaper-targets\wasm-guests")
$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent
# Each guest is its own workspace (the root Cargo.toml excludes them) built for wasm32-wasip2.
$guests = @(
  @{ Crate = "plugins\docker"; Wasm = "docker.wasm"; Into = "plugins\docker\docker.wasm" },
  @{ Crate = "plugins\hello"; Wasm = "hello.wasm"; Into = "plugins\hello\hello.wasm" },
  @{ Crate = "plugins\weather\component"; Wasm = "weather.wasm"; Into = "plugins\weather\weather.wasm" },
  @{ Crate = "plugins\tailscale"; Wasm = "tailscale.wasm"; Into = "plugins\tailscale\tailscale.wasm" }
)
if (-not (rustup target list --installed | Select-String -SimpleMatch "wasm32-wasip2")) { rustup target add wasm32-wasip2 }
foreach ($guest in $guests) {
  cargo build --release --target wasm32-wasip2 --manifest-path "$repo\$($guest.Crate)\Cargo.toml" --target-dir $Target
  if ($LASTEXITCODE) { throw "building $($guest.Crate) failed" }
  Copy-Item -LiteralPath "$Target\wasm32-wasip2\release\$($guest.Wasm)" -Destination "$repo\$($guest.Into)" -Force
  Write-Host "$($guest.Into): $([math]::Round((Get-Item "$repo\$($guest.Into)").Length / 1KB)) KB"
}
