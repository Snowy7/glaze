# Finite destructive-path/integrity checks against a disposable extracted release.
param(
    [Parameter(Mandatory)][string]$Archive,
    [Parameter(Mandatory)][string]$TestRoot
)
. "$PSScriptRoot\release-common.ps1"
Assert-OrdinaryUser
$TestRoot = Get-FullPath $TestRoot
if (Test-Path -LiteralPath $TestRoot) { throw 'Choose a new test directory.' }
New-Item -ItemType Directory -Path $TestRoot | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::ExtractToDirectory((Resolve-Path -LiteralPath $Archive).Path, $TestRoot)
$source = Join-Path $TestRoot 'Glaze-0.1.0-windows-x64'
$profile = Join-Path $TestRoot 'profile'
$install = Join-Path $profile 'Local\Programs\Glaze'
function Must-Refuse([scriptblock]$Action, [string]$Name) {
    $refused = $false
    try { & $Action } catch { $refused = $true; Write-Host "PASS $Name : $($_.Exception.Message)" }
    if (-not $refused) { throw "Failed to refuse $Name" }
}
Must-Refuse { & "$source\install.ps1" -Isolated -ProfileRoot $profile -InstallDirectory $profile } 'profile-root installation'
$file = Join-Path $source 'payload\docs\release-test.md'
$original = [IO.File]::ReadAllBytes($file)
try {
    [IO.File]::AppendAllText($file, 'damaged')
    Must-Refuse { & "$source\install.ps1" -Isolated -ProfileRoot $profile -InstallDirectory $install } 'damaged payload'
    if (Test-Path -LiteralPath $install) { throw 'Damaged payload created an install.' }
} finally { [IO.File]::WriteAllBytes($file, $original) }
$data = Join-Path $profile 'Roaming\glaze'
New-Item -ItemType Directory -Path $data -Force | Out-Null
$config = Join-Path $data 'config.json'
$badConfig = [Text.Encoding]::UTF8.GetBytes('{ malformed user config')
[IO.File]::WriteAllBytes($config, $badConfig)
& "$source\install.ps1" -Isolated -ProfileRoot $profile -InstallDirectory $install
if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($config)) -ne [Convert]::ToBase64String($badConfig)) {
    throw 'Installer overwrote malformed user data.'
}
Write-Host 'PASS malformed configuration kept byte for byte'
Must-Refuse { & "$source\install.ps1" -Isolated -ProfileRoot $profile -InstallDirectory $install } 'existing installation'
# Exercise the real recovery gate with a controlled command that exits 1. Hide the real
# process inventory only in this test scope; the fixture never invokes real recovery/registry work.
$core = Join-Path $install 'glaze-core.exe'
$savedCore = Join-Path $install 'glaze-core.original.exe'
Move-Item -LiteralPath $core -Destination $savedCore
Add-Type -TypeDefinition 'public static class GlazeRestoreRefusal { public static int Main(string[] args) { return 1; } }' `
    -OutputAssembly $core -OutputType ConsoleApplication
$statePath = Join-Path $install 'glaze-install.json'
$stateBytes = [IO.File]::ReadAllBytes($statePath)
$state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
$state.isolated = $false
$state | ConvertTo-Json | Set-Content -LiteralPath $statePath -Encoding UTF8
function Get-Process { param($Name, $ErrorAction) @(); }
try {
    Must-Refuse { & "$install\uninstall.ps1" -KeepUserData } 'failed restore command'
    if (-not (Test-Path -LiteralPath $config) -or -not (Test-Path -LiteralPath $core)) { throw 'Restore failure deleted files.' }
    Write-Host 'PASS restore exit 1 kept installation and user data'
} finally {
    Remove-Item Function:Get-Process
    [IO.File]::WriteAllBytes($statePath, $stateBytes)
    Remove-Item -LiteralPath $core -Force
    Move-Item -LiteralPath $savedCore -Destination $core
}
function Read-Host { param($Prompt) ''; }
& "$install\uninstall.ps1"
if ((Test-Path -LiteralPath $install) -or -not (Test-Path -LiteralPath $config)) { throw 'Default uninstall did not preserve settings.' }
Write-Host 'PASS empty deletion answer retained user data'
& "$source\install.ps1" -Isolated -ProfileRoot $profile -InstallDirectory $install
function Read-Host { param($Prompt) 'y'; }
& "$install\uninstall.ps1"
if ((Test-Path -LiteralPath $install) -or (Test-Path -LiteralPath $data)) { throw 'Confirmed uninstall did not remove only the intended data.' }
if (-not (Test-Path -LiteralPath $source)) { throw 'Uninstall touched the extracted source.' }
Write-Host 'PASS explicit deletion removed only Glaze roaming data'
Write-Host 'PASS installer safety checks'
