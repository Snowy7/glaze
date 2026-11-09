# Full v0.1 gate. Run through a child PowerShell so cargo warnings stay native stderr.
param(
    [string]$Target = $(if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'C:\glaze-targets\check' }),
    [string]$StillkitPath = (Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'stillkit')
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$StillkitPath = (Resolve-Path -LiteralPath $StillkitPath).Path
$Target = [IO.Path]::GetFullPath($Target)
New-Item -ItemType Directory -Path $Target -Force | Out-Null
$failed = [Collections.Generic.List[string]]::new()
$oldTarget = $env:CARGO_TARGET_DIR
$oldBackend = $env:CARGO_PROFILE_DEV_CODEGEN_BACKEND

function Invoke-Gate {
    param([string]$Name, [string]$Directory, [string]$BuildDirectory, [string]$Command, [string[]]$Arguments)
    Write-Host "`n=== $Name ==="
    Push-Location -LiteralPath $Directory
    try {
        $env:CARGO_TARGET_DIR = Join-Path $Target $BuildDirectory
        $executable = Get-Command $Command -ErrorAction Stop
        # Windows PowerShell turns redirected native warnings into ErrorRecords.
        $ErrorActionPreference = 'Continue'
        & $executable @Arguments
        $code = $LASTEXITCODE
        if ($code) {
            $failed.Add($Name)
            Write-Host "$Name FAILED (exit $code)"
        } else {
            Write-Host "$Name PASSED"
        }
    } catch {
        $failed.Add($Name)
        Write-Host "$Name FAILED: $_"
    } finally {
        Pop-Location
    }
}

function Test-CpuImports {
    # Resolve the current CPU test artifact from Cargo, rather than choosing a stale hash.
    Push-Location -LiteralPath $StillkitPath
    try {
        $env:CARGO_TARGET_DIR = Join-Path $Target 'stillkit-build'
        $ErrorActionPreference = 'Continue'
        $messages = & cargo test -p stillkit --features golden,serde --test no_gpu_driver --no-run --message-format=json
        if ($LASTEXITCODE) { throw 'Could not build the CPU import-check executable' }
        $artifact = $messages | Where-Object { $_.StartsWith('{') } | ForEach-Object { $_ | ConvertFrom-Json } |
            Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'no_gpu_driver' -and $_.executable } |
            Select-Object -Last 1
        if (-not $artifact) { throw 'Cargo did not report the CPU test executable' }
        $readobj = Get-Command llvm-readobj.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -First 1
        if (-not $readobj) {
            $readobj = Join-Path $env:ProgramFiles 'LLVM\bin\llvm-readobj.exe'
        }
        if (-not (Test-Path -LiteralPath $readobj)) { throw 'Put llvm-readobj.exe on PATH or install LLVM under Program Files' }
        $imports = & $readobj --coff-imports $artifact.executable
        if ($LASTEXITCODE) { throw 'llvm-readobj could not inspect the CPU executable' }
        $names = @($imports | Select-String '^\s*Name: (.+\.dll)\s*$' | ForEach-Object { $_.Matches[0].Groups[1].Value })
        if (-not $names.Count) { throw 'The CPU executable has no readable DLL import table' }
        $gpu = @($names | Where-Object { $_ -match '^(d3d12|dxcompiler|vulkan-1|opengl32|nv.*umx|nvgpucomp64|amdxc64|amdxx64|atidx.*|igd.*)\.dll$' })
        if ($gpu.Count) { throw "CPU executable imports GPU libraries: $($gpu -join ', ')" }
        Write-Host "CPU import table PASSED ($($names.Count) DLL imports; no GPU vendor driver or Direct3D 12)"
    } finally {
        Pop-Location
    }
}

try {
    if ($env:GLAZE_BLESS -or $env:STILLKIT_BLESS) { throw 'Unset GLAZE_BLESS and STILLKIT_BLESS before checking goldens' }
    # Cranelift can abort a whole Windows test binary when one assertion fails.
    $env:CARGO_PROFILE_DEV_CODEGEN_BACKEND = 'llvm'
    $patchFile = Join-Path $Target 'stillkit-patch.toml'
    $stillkitCrate = (Join-Path $StillkitPath 'crates\stillkit').Replace('\', '/')
    [IO.File]::WriteAllText($patchFile, "[patch.`"https://github.com/Snowy7/stillkit`"]`nstillkit = { path = `"$stillkitCrate`" }`n")
    $sdk = Join-Path $repo 'packages\plugin-sdk-ts'
    Invoke-Gate 'TypeScript SDK dependencies' $repo 'plugin-build' pnpm @('--dir', $sdk, 'install', '--frozen-lockfile', '--prefer-offline')
    Invoke-Gate 'TypeScript demo dependencies' $repo 'plugin-build' pnpm @('--dir', (Join-Path $repo 'plugins\hello-ts'), 'install', '--frozen-lockfile', '--prefer-offline')
    Invoke-Gate 'TypeScript SDK typecheck' $repo 'plugin-build' pnpm @('--dir', $sdk, 'run', 'typecheck')
    Invoke-Gate 'TypeScript SDK tests' $repo 'plugin-build' pnpm @('--dir', $sdk, 'run', 'test')
    # Workspace runtime tests consume these components, so build them before running tests.
    # The release wrapper also builds hello-ts against the host's canonical WIT.
    Invoke-Gate 'WASM plugins (Rust and TypeScript)' $repo 'wasm-build' powershell @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $PSScriptRoot 'release\build-plugins.ps1'), '-Target', (Join-Path $Target 'wasm-build'))
    Invoke-Gate 'Glaze workspace tests' $repo 'build' cargo @('--config', $patchFile, 'test', '--workspace', '--no-fail-fast')
    # These component crates have native unit tests. hello and weather/component are WASM-only
    # and have no tests; the build above validates them on their actual target.
    foreach ($plugin in @('discord', 'obs', 'grok', 'docker', 'tailscale', 't3')) {
        Invoke-Gate "Plugin $plugin tests" $repo 'plugin-build' cargo @('test', '--manifest-path', "plugins\$plugin\Cargo.toml", '--no-fail-fast')
    }

    $oldAppData = $env:APPDATA
    $oldLocalAppData = $env:LOCALAPPDATA
    try {
        $env:APPDATA = Join-Path $Target 'plugin-profile\Roaming'
        $env:LOCALAPPDATA = Join-Path $Target 'plugin-profile\Local'
        New-Item -ItemType Directory -Path $env:APPDATA, $env:LOCALAPPDATA -Force | Out-Null
        Invoke-Gate 'Plugin host linker compatibility' $repo 'build' cargo @('--config', $patchFile, 'run', '-p', 'glaze-plugins', '--', '--compile', (Join-Path $repo 'plugins'))
        Invoke-Gate 'Hello plugin host lifecycle' $repo 'build' cargo @('--config', $patchFile, 'run', '-p', 'glaze-plugins', '--', '--bench', (Join-Path $repo 'plugins\hello'))
    } finally {
        $env:APPDATA = $oldAppData
        $env:LOCALAPPDATA = $oldLocalAppData
    }

    Invoke-Gate 'stillkit all-target check' $StillkitPath 'stillkit-build' cargo @('check', '-p', 'stillkit', '--all-targets', '--features', 'golden')
    Invoke-Gate 'stillkit workspace tests' $StillkitPath 'stillkit-build' cargo @('test', '--workspace', '--no-fail-fast', '--features', 'stillkit/golden,stillkit/serde')
    try { Test-CpuImports } catch { $failed.Add('stillkit CPU import table'); Write-Host "CPU import table FAILED: $_" }
    Invoke-Gate 'stillkit Clippy (including GPU code)' $StillkitPath 'stillkit-build' cargo @('clippy', '--workspace', '--all-targets', '--features', 'stillkit/golden,stillkit/serde,stillkit/gpu')
    Invoke-Gate 'Glaze workspace Clippy' $repo 'build' cargo @('--config', $patchFile, 'clippy', '--workspace')
} finally {
    $env:CARGO_TARGET_DIR = $oldTarget
    $env:CARGO_PROFILE_DEV_CODEGEN_BACKEND = $oldBackend
}

if ($failed.Count) {
    Write-Host "`nCHECK FAILED: $($failed -join '; ')"
    exit 1
}
Write-Host "`nCHECK PASSED: workspace, plugins, stillkit, CPU imports and Clippy"
