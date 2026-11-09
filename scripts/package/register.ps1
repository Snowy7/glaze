param(
    [Parameter(Mandatory)][string]$ExternalLocation,
    [Parameter(Mandatory)][string]$PackagePath,
    [switch]$Development
)
. "$PSScriptRoot\common.ps1"
$ExternalLocation = (Resolve-Path -LiteralPath $ExternalLocation).Path.TrimEnd('\')
$PackagePath = (Resolve-Path -LiteralPath $PackagePath).Path
foreach ($exe in 'glaze-shell.exe', 'glaze-core.exe') {
    if (-not (Test-Path -LiteralPath (Join-Path $ExternalLocation $exe))) { throw "Missing $exe at $ExternalLocation" }
}
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($PackagePath)
try {
    $reader = New-Object IO.StreamReader ($archive.GetEntry('AppxManifest.xml').Open())
    try { [xml]$manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
} finally { $archive.Dispose() }
if ($manifest.Package.Identity.Name -ne (Get-PackageManifest).Package.Identity.Name) { throw 'Unexpected package identity' }
$installed = Get-AppxPackage -Name $manifest.Package.Identity.Name
if ($installed) {
    [Windows.Management.Deployment.PackageManager, Windows.Management.Deployment, ContentType = WindowsRuntime] | Out-Null
    $manager = New-Object Windows.Management.Deployment.PackageManager
    $package = $manager.FindPackageForUser('', $installed.PackageFullName)
    [xml]$registered = Get-Content -LiteralPath (Join-Path $installed.InstallLocation 'AppxManifest.xml') -Raw
    if ($package.EffectiveExternalLocation.Path.TrimEnd('\') -eq $ExternalLocation -and $registered.OuterXml -eq $manifest.OuterXml) {
        Write-Host "Identity already registered at $ExternalLocation; keeping consent and registration."
        return
    }
    if ([version]$manifest.Package.Identity.Version -le [version]$installed.Version) {
        throw "Identity changed. Build a higher package version than $($installed.Version), then register it. Existing registration was preserved."
    }
}
Assert-GlazeStopped
if ($Development) {
    $mode = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock' -ErrorAction SilentlyContinue
    if ($mode.AllowDevelopmentWithoutDevLicense -ne 1) {
        throw 'Development registration needs existing Windows Developer Mode. No machine policy was changed. Use a production-trusted signed MSIX instead.'
    }
    # AppxDeployment checks machine trust even for per-user signed MSIX installs. With only
    # current-user development trust, use Windows' documented loose-layout registration.
    $signature = Get-AuthenticodeSignature -LiteralPath $PackagePath
    if (-not $signature.SignerCertificate -or
        $signature.SignerCertificate.Subject -ne $manifest.Package.Identity.Publisher -or
        -not (Test-Path -LiteralPath "Cert:\CurrentUser\TrustedPeople\$($signature.SignerCertificate.Thumbprint)")) {
        throw 'The development MSIX must be signed by a publisher certificate in CurrentUser\TrustedPeople.'
    }
    $hash = (Get-FileHash -LiteralPath $PackagePath -Algorithm SHA256).Hash
    $layout = Join-Path $env:LOCALAPPDATA "glaze\identity\$($manifest.Package.Identity.Version)\$hash"
    if (-not (Test-Path -LiteralPath (Join-Path $layout 'AppxManifest.xml'))) {
        # A release user's PC needs no Windows SDK. The signed MSIX is a ZIP archive.
        New-Item -ItemType Directory -Path $layout -Force | Out-Null
        [IO.Compression.ZipFile]::ExtractToDirectory($PackagePath, $layout)
    }
    Add-AppxPackage -Register -Path (Join-Path $layout 'AppxManifest.xml') -ExternalLocation $ExternalLocation
} else {
    Add-AppxPackage -Path $PackagePath -ExternalLocation $ExternalLocation
}
Write-Host "Registered $($manifest.Package.Identity.Name) at $ExternalLocation. Restart core and shell to acquire identity."
