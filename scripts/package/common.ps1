$ErrorActionPreference = 'Stop'

function Get-PackageManifest {
    [xml](Get-Content -LiteralPath (Join-Path $PSScriptRoot 'AppxManifest.xml') -Raw)
}

function Get-SdkTool([string]$Name) {
    $root = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots').KitsRoot10
    $versions = Get-ChildItem -LiteralPath (Join-Path $root 'bin') -Directory |
        Where-Object { $_.Name -match '^10\.0\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending
    foreach ($version in $versions) {
        $tool = Join-Path $version.FullName "x64\$Name"
        if (Test-Path -LiteralPath $tool) { return $tool }
    }
    throw "Windows SDK tool $Name is missing. Install the Windows 10/11 SDK."
}

function Invoke-PackageTool([string]$Tool, [string[]]$Arguments) {
    & $Tool @Arguments | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$Tool failed ($LASTEXITCODE)" }
}

# Changing a live process's registration does not change that process's identity.
function Assert-GlazeStopped {
    $running = @(Get-Process glaze-core, glaze-shell -ErrorAction SilentlyContinue)
    if ($running.Count) {
        $running | Select-Object Id, ProcessName, Path | Format-Table -AutoSize | Out-Host
        throw 'Stop core and shell before changing package registration (deploy-local.ps1 does this under the desktop lock).'
    }
}
