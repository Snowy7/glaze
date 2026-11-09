# Removes this user's package registration only. The exes and development signing key remain.
param(
    [string]$AppDirectory = (Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'target\release'),
    # Release uninstall passes this only after glaze-core --restore has exited successfully.
    [switch]$WindowsRestored
)
. "$PSScriptRoot\common.ps1"
# Restore opt-in folder associations before removing identity or an installed app directory.
# The Files CLI also switches its config flag off, so a later core launch cannot re-enable it.
$files = Join-Path $AppDirectory 'glaze-files.exe'
if (-not $WindowsRestored -and (Test-Path -LiteralPath $files)) {
    $restore = Start-Process -FilePath $files -ArgumentList '--restore-routing' -Wait -PassThru -WindowStyle Hidden
    if ($restore.ExitCode -ne 0) { throw 'Folder routing could not be restored; package registration was left intact.' }
} elseif (-not $WindowsRestored -and (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA 'glaze\undo-folders.json'))) {
    throw 'Restore folder routing with glaze-files --restore-routing before unregistering this installation.'
}
$installed = Get-AppxPackage -Name (Get-PackageManifest).Package.Identity.Name
if ($installed) {
    Assert-GlazeStopped
    $layout = [IO.Path]::GetFullPath($installed.InstallLocation)
    Remove-AppxPackage -Package $installed.PackageFullName
    $developmentRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'glaze\identity')).TrimEnd('\') + '\'
    if ($installed.IsDevelopmentMode -and $layout.StartsWith($developmentRoot, [StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $layout)) {
        Remove-Item -LiteralPath $layout -Recurse -Force
    }
    Write-Host 'Glaze identity removed; the next launch uses notification snapshots.'
} else { Write-Host 'Glaze identity is already unregistered.' }
