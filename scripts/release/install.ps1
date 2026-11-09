param(
    [string]$InstallDirectory,
    [switch]$NoLaunch,
    [ValidateSet('Auto', 'Skip')][string]$Identity = 'Auto',
    # A payload-only test: never touches the real identity, startup, shortcuts or registry.
    [switch]$Isolated,
    [string]$ProfileRoot
)
. "$PSScriptRoot\release-common.ps1"

if (-not [Environment]::Is64BitOperatingSystem -or [Environment]::OSVersion.Version.Build -lt 19041) {
    throw 'Glaze 0.1.0 requires 64-bit Windows 10 2004 or Windows 11.'
}
if ($Isolated) {
    if (-not $ProfileRoot -or -not $InstallDirectory) { throw '-Isolated requires -ProfileRoot and -InstallDirectory.' }
    $ProfileRoot = Get-FullPath $ProfileRoot
    $env:APPDATA = Join-Path $ProfileRoot 'Roaming'
    $env:LOCALAPPDATA = Join-Path $ProfileRoot 'Local'
    $NoLaunch = $true
} else {
    Assert-OrdinaryUser
    if ($ProfileRoot) { throw '-ProfileRoot is only supported with -Isolated.' }
}
foreach ($directory in $env:APPDATA,$env:LOCALAPPDATA) {
    if (-not $directory) { throw 'APPDATA and LOCALAPPDATA must be set.' }
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
}
$programs = Join-Path $env:LOCALAPPDATA 'Programs'
if (-not $InstallDirectory) { $InstallDirectory = Join-Path $programs 'Glaze' }
$InstallDirectory = Assert-ChildPath $InstallDirectory $(if ($Isolated) { $ProfileRoot } else { $programs })
$payload = Join-Path $PSScriptRoot 'payload'
Assert-PlainTree $payload
$inventory = Get-Content -LiteralPath (Join-Path $payload 'release-files.json') -Raw | ConvertFrom-Json
if ($inventory.version -ne '0.1.0') { throw 'Unexpected release version.' }
foreach ($file in $inventory.files) {
    $path = Assert-ChildPath (Join-Path $payload $file.path) $payload
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $file.sha256) {
        throw "Release file is missing or damaged: $($file.path). Extract the zip again."
    }
}
if (Test-Path -LiteralPath $InstallDirectory) {
    if (Get-ChildItem -LiteralPath $InstallDirectory -Force | Select-Object -First 1) {
        $previous = Read-InstallState $InstallDirectory
        if ([bool]$previous.isolated -ne [bool]$Isolated) { throw 'Cannot change an installation between real and isolated mode.' }
        throw 'Glaze is already installed here. Uninstall it first (keep your settings), then install this release.'
    }
}
if (-not $Isolated -and @(Get-Process -Name 'glaze-*' -ErrorAction SilentlyContinue).Count) {
    throw 'Quit Glaze before installing. Another running installation was left untouched.'
}
if (-not $Isolated -and (Test-Path -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Glaze')) {
    throw 'An existing Glaze uninstall entry exists. Uninstall that copy first.'
}
New-Item -ItemType Directory -Path $InstallDirectory -Force | Out-Null
Copy-Item -Path (Join-Path $payload '*') -Destination $InstallDirectory -Recurse -Force
$state = [ordered]@{
    product = 'Glaze'; version = '0.1.0'; directory = $InstallDirectory; isolated = [bool]$Isolated
    profileRoot = $ProfileRoot; roaming = $env:APPDATA; local = $env:LOCALAPPDATA
    identity = $false; certificate = ''; certificateImported = $false; shortcut = ''
}
$statePath = Join-Path $InstallDirectory 'glaze-install.json'
function Save-State { $state | ConvertTo-Json | Set-Content -LiteralPath $statePath -Encoding UTF8 }
Save-State
# An empty document is the config crate's canonical default input. Never replace user data.
$configDirectory = Join-Path $env:APPDATA 'glaze'
New-Item -ItemType Directory -Path $configDirectory -Force | Out-Null
$configPath = Join-Path $configDirectory 'config.json'
if (-not (Test-Path -LiteralPath $configPath)) {
    $configFile = [IO.File]::Open($configPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write)
    try { $bytes = [Text.Encoding]::UTF8.GetBytes("{}`n"); $configFile.Write($bytes, 0, $bytes.Length) }
    finally { $configFile.Dispose() }
}
if (-not $Isolated) {
    # Write recovery/uninstall entry before optional integration work, so a partial install is removable.
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Glaze'
    New-Item -Path $key -Force | Out-Null
    $uninstall = 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "' + (Join-Path $InstallDirectory 'uninstall.ps1') + '"'
    foreach ($entry in @{
        DisplayName = 'Glaze'; DisplayVersion = '0.1.0'; Publisher = 'Glaze'; InstallLocation = $InstallDirectory
        DisplayIcon = (Join-Path $InstallDirectory 'glaze-studio.exe'); UninstallString = $uninstall
        InstallDate = (Get-Date -Format 'yyyyMMdd')
    }.GetEnumerator()) {
        New-ItemProperty -LiteralPath $key -Name $entry.Key -Value $entry.Value -PropertyType String -Force | Out-Null
    }
    foreach ($name in 'NoModify','NoRepair') {
        New-ItemProperty -LiteralPath $key -Name $name -Value 1 -PropertyType DWord -Force | Out-Null
    }
    if ($Identity -eq 'Auto') {
        $identityRoot = Join-Path $InstallDirectory 'identity'
        $package = Join-Path $identityRoot 'glaze.Desktop.msix'
        try {
            $signature = Get-AuthenticodeSignature -LiteralPath $package
            $certificate = New-Object Security.Cryptography.X509Certificates.X509Certificate2 (Join-Path $identityRoot 'glaze-development.cer')
            [xml]$manifest = Get-Content -LiteralPath (Join-Path $identityRoot 'AppxManifest.xml') -Raw
            if (-not $signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -ne $certificate.Thumbprint -or
                $certificate.Subject -ne $manifest.Package.Identity.Publisher) { throw 'Sparse package certificate does not match its signature/publisher.' }
            $state.certificate = $certificate.Thumbprint
            if (-not (Test-Path -LiteralPath "Cert:\CurrentUser\TrustedPeople\$($certificate.Thumbprint)")) {
                Import-Certificate -FilePath (Join-Path $identityRoot 'glaze-development.cer') -CertStoreLocation Cert:\CurrentUser\TrustedPeople | Out-Null
                $state.certificateImported = $true
                Save-State
            }
            try { & "$identityRoot\register.ps1" -PackagePath $package -ExternalLocation $InstallDirectory }
            catch {
                $mode = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock' -ErrorAction SilentlyContinue
                if (-not $mode -or $mode.PSObject.Properties.Name -notcontains 'AllowDevelopmentWithoutDevLicense' -or
                    $mode.AllowDevelopmentWithoutDevLicense -ne 1) { throw }
                & "$identityRoot\register.ps1" -PackagePath $package -ExternalLocation $InstallDirectory -Development
            }
            $state.identity = $true
        } catch {
            Write-Warning "Package identity unavailable: $($_.Exception.Message) Glaze will run with notification snapshot fallback; live notification access is unavailable. See docs\release.md."
            if ($state.certificateImported) {
                Remove-Item -LiteralPath "Cert:\CurrentUser\TrustedPeople\$($state.certificate)" -Force
                $state.certificateImported = $false
            }
        }
        Save-State
    }
    Invoke-GlazeCommand (Join-Path $InstallDirectory 'glaze-core.exe') @('--autostart','on') | Out-Host
    $menu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
    New-Item -ItemType Directory -Path $menu -Force | Out-Null
    $state.shortcut = Join-Path $menu 'Glaze Studio.lnk'
    $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($state.shortcut)
    $shortcut.TargetPath = Join-Path $InstallDirectory 'glaze-studio.exe'
    $shortcut.WorkingDirectory = $InstallDirectory
    $shortcut.Description = 'Glaze 0.1.0 Studio'
    $shortcut.Save()
    Save-State
}
# Cache is machine-specific: compile the bundled WASM here, never ship this PC's AOT cache.
try { Invoke-GlazeCommand (Join-Path $InstallDirectory 'glaze-plugins.exe') @('--compile', ('"' + (Join-Path $InstallDirectory 'plugins') + '"')) -TimeoutSeconds 300 | Out-Host }
catch { Write-Warning "Plugin precompile failed; plugins retry on first use: $($_.Exception.Message)" }
Write-Host "Glaze 0.1.0 installed at $InstallDirectory"
if ($Isolated) { Write-Host 'Isolated payload install: Windows integration was intentionally skipped.' }
elseif (-not $NoLaunch) {
    Start-Process -FilePath (Join-Path $InstallDirectory 'glaze-core.exe') -WorkingDirectory $InstallDirectory
    Start-Process -FilePath (Join-Path $InstallDirectory 'glaze-studio.exe') -WorkingDirectory $InstallDirectory
}
