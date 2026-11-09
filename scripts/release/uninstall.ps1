param(
    # Explicit automation keeps data. Interactive removal still requires a separate yes answer.
    [switch]$KeepUserData
)
. "$PSScriptRoot\release-common.ps1"
$directory = Get-FullPath $PSScriptRoot
$state = Read-InstallState $directory
if (-not $state.isolated) { Assert-OrdinaryUser }
$parent = if ($state.isolated) { $state.profileRoot } else { Join-Path $env:LOCALAPPDATA 'Programs' }
$directory = Assert-ChildPath $directory $parent
Assert-PlainTree $directory
if ($state.isolated) {
    $env:APPDATA = $state.roaming
    $env:LOCALAPPDATA = $state.local
} else {
    $other = @(Get-Process -Name 'glaze-*' -ErrorAction SilentlyContinue | Where-Object {
        -not $_.Path -or -not $_.Path.StartsWith($directory + '\', [StringComparison]::OrdinalIgnoreCase)
    })
    if ($other.Count) { throw 'Another Glaze installation is running. Quit it before uninstalling this copy.' }
    # Restore first. Do not unregister, kill or delete anything if recovery fails.
    Invoke-GlazeRestore $directory
    Invoke-GlazeCommand (Join-Path $directory 'glaze-core.exe') @('--autostart','off') | Out-Host
}
Stop-InstallProcesses $directory
if (-not $state.isolated) {
    $installed = Get-AppxPackage -Name 'glaze.Desktop'
    if ($installed) {
        [Windows.Management.Deployment.PackageManager, Windows.Management.Deployment, ContentType = WindowsRuntime] | Out-Null
        $manager = New-Object Windows.Management.Deployment.PackageManager
        $package = $manager.FindPackageForUser('', $installed.PackageFullName)
        if ((Get-FullPath $package.EffectiveExternalLocation.Path) -eq $directory) {
            & "$directory\identity\unregister.ps1" -AppDirectory $directory -WindowsRestored
        }
    }
    if ($state.shortcut -and (Test-Path -LiteralPath $state.shortcut)) {
        $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($state.shortcut)
        if ((Get-FullPath $shortcut.TargetPath) -eq (Join-Path $directory 'glaze-studio.exe')) {
            Remove-Item -LiteralPath $state.shortcut -Force
        }
    }
    if ($state.certificateImported -and $state.certificate) {
        # Remove only the exact leaf this install added, after its package is gone.
        if (-not (Get-AppxPackage -Name 'glaze.Desktop')) {
            Remove-Item -LiteralPath "Cert:\CurrentUser\TrustedPeople\$($state.certificate)" -Force -ErrorAction SilentlyContinue
        }
    }
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Glaze'
    $entry = Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
    if ($entry -and (Get-FullPath $entry.InstallLocation) -eq $directory) { Remove-Item -LiteralPath $key -Recurse -Force }
}
$data = Join-Path $env:APPDATA 'glaze'
if (-not $KeepUserData -and (Test-Path -LiteralPath $data)) {
    $answer = Read-Host "Delete your Glaze settings, themes and plugin data at $data ? [y/N]"
    if ($answer -ceq 'y' -or $answer -ceq 'yes') {
        $data = Assert-ChildPath $data $env:APPDATA
        Assert-PlainTree $data
        Remove-Item -LiteralPath $data -Recurse -Force
    }
}
# Keep local journals/caches too: a failed or interrupted restore must remain recoverable.
Set-Location -LiteralPath ([IO.Path]::GetTempPath())
Remove-Item -LiteralPath $directory -Recurse -Force
Write-Host 'Glaze uninstalled. User data was kept unless you explicitly chose to delete it.'
