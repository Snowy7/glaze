param(
    [string]$OutputDirectory = (Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'target\package'),
    [version]$Version,
    [string]$ExternalLocation,
    [string]$CertificateThumbprint,
    [string]$TimestampUrl
)
. "$PSScriptRoot\common.ps1"
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$staging = Join-Path $OutputDirectory ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $staging 'Assets') | Out-Null
try {
    $manifest = Get-PackageManifest
    if (-not $Version -and $ExternalLocation) {
        $installed = Get-AppxPackage -Name $manifest.Package.Identity.Name
        if ($installed) {
            [Windows.Management.Deployment.PackageManager, Windows.Management.Deployment, ContentType = WindowsRuntime] | Out-Null
            $manager = New-Object Windows.Management.Deployment.PackageManager
            $current = $manager.FindPackageForUser('', $installed.PackageFullName)
            [xml]$registered = Get-Content -LiteralPath (Join-Path $installed.InstallLocation 'AppxManifest.xml') -Raw
            $Version = [version]$installed.Version
            $manifest.Package.Identity.Version = $Version.ToString(4)
            if ($registered.OuterXml -ne $manifest.OuterXml -or
                $current.EffectiveExternalLocation.Path.TrimEnd('\') -ne [IO.Path]::GetFullPath($ExternalLocation).TrimEnd('\')) {
                if ($Version.Revision -eq 65535) { throw 'Specify a higher package version; revision is exhausted.' }
                $Version = [version]::new($Version.Major, $Version.Minor, $Version.Build, $Version.Revision + 1)
            }
        }
    }
    if ($Version) { $manifest.Package.Identity.Version = $Version.ToString(4) }
    $manifest.Save((Join-Path $staging 'AppxManifest.xml'))
    # Use the app mark for Windows identity; Files embeds its separate folder mark.
    Add-Type -AssemblyName System.Drawing
    $logoRoot = Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'assets\logo\png'
    foreach ($asset in @(@('StoreLogo', 50), @('Square44x44Logo', 44), @('Square150x150Logo', 150))) {
        $bitmap = New-Object Drawing.Bitmap ([int]$asset[1]), ([int]$asset[1])
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $size = [int]$asset[1]
        $sourceSize = if ($size -le 32) { 32 } elseif ($size -le 64) { 64 } else { 256 }
        $source = [Drawing.Image]::FromFile((Join-Path $logoRoot "mark-$sourceSize.png"))
        try {
            $graphics.Clear([Drawing.Color]::Transparent)
            $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.DrawImage($source, 0, 0, $size, $size)
            $bitmap.Save((Join-Path $staging "Assets\$($asset[0]).png"), [Drawing.Imaging.ImageFormat]::Png)
        } finally { $source.Dispose(); $graphics.Dispose(); $bitmap.Dispose() }
    }
    $package = Join-Path $OutputDirectory 'glaze.Desktop.msix'
    Invoke-PackageTool (Get-SdkTool 'makeappx.exe') @('pack', '/o', '/nv', '/d', $staging, '/p', $package)
    & "$PSScriptRoot\sign.ps1" -PackagePath $package -CertificateThumbprint $CertificateThumbprint -TimestampUrl $TimestampUrl
    $package
} finally {
    # $staging is a GUID directory created directly under this resolved output directory.
    if ((Split-Path $staging -Parent) -ne $OutputDirectory) { throw 'Invalid staging path' }
    Remove-Item -LiteralPath $staging -Recurse -Force
}
