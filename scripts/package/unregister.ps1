# Removes this user's package registration only. The exes and development signing key remain.
. "$PSScriptRoot\common.ps1"
$installed = Get-AppxPackage -Name (Get-PackageManifest).Package.Identity.Name
if ($installed) {
    Assert-MypaperStopped
    $layout = [IO.Path]::GetFullPath($installed.InstallLocation)
    Remove-AppxPackage -Package $installed.PackageFullName
    $developmentRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'mypaper\identity')).TrimEnd('\') + '\'
    if ($installed.IsDevelopmentMode -and $layout.StartsWith($developmentRoot, [StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $layout)) {
        Remove-Item -LiteralPath $layout -Recurse -Force
    }
    Write-Host 'mypaper identity removed; the next launch uses notification snapshots.'
} else { Write-Host 'mypaper identity is already unregistered.' }
