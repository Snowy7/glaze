# Packages an existing release build. No SDK/toolchain is needed on the installing PC.
param(
    [string]$From = $(if ($env:CARGO_TARGET_DIR) { Join-Path $env:CARGO_TARGET_DIR 'release' } else { Join-Path $PSScriptRoot '..\..\target\release' }),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '..\..\dist'),
    [string]$CrtDirectory,
    [string]$CertificateThumbprint,
    [string]$TimestampUrl
)
. "$PSScriptRoot\release-common.ps1"
$repo = Get-FullPath (Join-Path $PSScriptRoot '..\..')
$From = (Resolve-Path -LiteralPath $From).Path
$OutputDirectory = Get-FullPath $OutputDirectory
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$name = 'Glaze-0.1.0-windows-x64'
$stage = Join-Path $OutputDirectory ($name + '-' + [Guid]::NewGuid().ToString('N'))
$root = Join-Path $stage $name
$payload = Join-Path $root 'payload'
New-Item -ItemType Directory -Path $payload -Force | Out-Null
try {
    foreach ($file in @('glaze-core.exe','glaze-shell.exe','glaze-wallpaper.exe','glaze-studio.exe',
        'glaze-files.exe','glaze-index.exe','glaze-plugins.exe','glaze-native-host.exe','glaze-styler.exe','glaze_app_styler.dll')) {
        Copy-Item -LiteralPath (Join-Path $From $file) -Destination $payload
    }
    # These folders are resolved by glaze-theme and glaze-widgets beside current_exe().
    foreach ($folder in 'themes','effects') {
        $source = Join-Path $repo $folder
        Assert-PlainTree $source
        Copy-Item -LiteralPath $source -Destination $payload -Recurse
    }
    # Manifests are the source of truth. Don't ship guest source, test evidence or AOT caches.
    $plugins = Join-Path $payload 'plugins'
    New-Item -ItemType Directory -Path $plugins | Out-Null
    foreach ($manifest in Get-ChildItem -Path (Join-Path $repo 'plugins\*\plugin.json') -File) {
        $plugin = Get-Content -LiteralPath $manifest.FullName -Raw | ConvertFrom-Json
        $destination = Join-Path $plugins $plugin.id
        New-Item -ItemType Directory -Path $destination | Out-Null
        Copy-Item -LiteralPath $manifest.FullName -Destination $destination
        foreach ($property in 'component','command') {
            if ($plugin.PSObject.Properties.Name -contains $property) {
                $relative = $plugin.$property
                $source = Assert-ChildPath (Join-Path $manifest.DirectoryName $relative) $manifest.DirectoryName
                if ($property -eq 'command') {
                    $source = Join-Path $From $relative
                }
                Copy-Item -LiteralPath $source -Destination (Join-Path $destination $relative)
            }
        }
        foreach ($file in Get-ChildItem -LiteralPath $manifest.DirectoryName -File | Where-Object { $_.Name -match '^(LICENSE|NOTICE|COPYING)' }) {
            Copy-Item -LiteralPath $file.FullName -Destination $destination
        }
    }
    # MSVC builds import the CRT. App-local copies avoid assuming VS/redist was installed.
    if (-not $CrtDirectory) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
        $installations = @(& $vswhere -all -products '*' -property installationPath)
        if ($LASTEXITCODE -ne 0 -or -not $installations.Count) { throw 'Pass -CrtDirectory pointing to the x64 Microsoft.VC*.CRT redistributable folder.' }
        $crt = @(foreach ($vs in $installations) {
            $redist = Join-Path $vs 'VC\Redist\MSVC'
            if (Test-Path -LiteralPath $redist) {
                Get-ChildItem -Path (Join-Path $redist '*\x64\Microsoft.VC*.CRT') -Directory
            }
        }) | Sort-Object FullName -Descending | Select-Object -First 1
        if (-not $crt) { throw 'MSVC x64 CRT redistributable directory was not found. Pass -CrtDirectory.' }
        $CrtDirectory = $crt.FullName
    }
    foreach ($dll in Get-ChildItem -LiteralPath $CrtDirectory -Filter '*.dll' -File) {
        Copy-Item -LiteralPath $dll.FullName -Destination $payload
    }
    if (-not (Test-Path -LiteralPath (Join-Path $payload 'vcruntime140.dll'))) { throw 'The x64 CRT redistributables are incomplete.' }
    # A native guest's application directory is its own plugin folder, not the host's.
    foreach ($native in Get-ChildItem -LiteralPath $plugins -Filter '*.exe' -Recurse -File) {
        foreach ($dll in Get-ChildItem -LiteralPath $CrtDirectory -Filter '*.dll' -File) {
            Copy-Item -LiteralPath $dll.FullName -Destination $native.DirectoryName
        }
    }
    & "$PSScriptRoot\audit-imports.ps1" -Directory $payload -Output (Join-Path $payload 'runtime-imports.json')
    $identity = Join-Path $payload 'identity'
    New-Item -ItemType Directory -Path $identity | Out-Null
    & "$repo\scripts\package\build.ps1" -OutputDirectory $identity -Version '0.1.0.0' `
        -CertificateThumbprint $CertificateThumbprint -TimestampUrl $TimestampUrl | Out-Host
    foreach ($file in 'register.ps1','unregister.ps1','common.ps1','AppxManifest.xml') {
        Copy-Item -LiteralPath (Join-Path $repo "scripts\package\$file") -Destination $identity
    }
    # A CA certificate is public too; retain the installer's fixed certificate filename.
    if ($CertificateThumbprint) {
        Export-Certificate -Cert (Get-Item "Cert:\CurrentUser\My\$CertificateThumbprint") `
            -FilePath (Join-Path $identity 'glaze-development.cer') -Force | Out-Null
    }
    foreach ($file in 'uninstall.ps1','uninstall.cmd','release-common.ps1') {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination $payload
    }
    foreach ($file in 'install.ps1','install.cmd','release-common.ps1') {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $file) -Destination $root
    }
    $docs = Join-Path $payload 'docs'
    New-Item -ItemType Directory -Path $docs | Out-Null
    foreach ($file in 'release.md','release-test.md') {
        Copy-Item -LiteralPath (Join-Path $repo "docs\$file") -Destination $docs
    }
    foreach ($file in 'restore.md','release-notes-v0.1.0.md') {
        if (Test-Path -LiteralPath (Join-Path $repo "docs\$file")) {
            Copy-Item -LiteralPath (Join-Path $repo "docs\$file") -Destination $docs
        }
    }
    if (Test-Path -LiteralPath (Join-Path $repo 'licenses')) {
        Copy-Item -LiteralPath (Join-Path $repo 'licenses') -Destination $payload -Recurse
    }
    foreach ($file in Get-ChildItem -LiteralPath $repo -File | Where-Object { $_.Name -match '^(LICENSE|NOTICE|THIRD[-_]PARTY|CHANGELOG)' }) {
        Copy-Item -LiteralPath $file.FullName -Destination $payload
    }
    @'
Glaze 0.1.0 for Windows x64

Extract this entire zip, then double-click install.cmd (do not run as administrator).
Installs to %LOCALAPPDATA%\Programs\Glaze. No Rust, SDK, repo checkout or paid certificate needed.
Read payload\docs\release.md for certificate trust, SmartScreen and identity fallback limitations.
Uninstall from Windows Settings > Apps, or the installed uninstall.cmd. Settings are kept by default.
If Windows blocks the download, inspect its source and SHA256 first; do not disable SmartScreen.
Run payload\docs\release-test.md on a second PC before calling this release qualified.
'@ | Set-Content -LiteralPath (Join-Path $root 'READ-ME.txt') -Encoding UTF8
    $files = @(Get-ChildItem -LiteralPath $payload -File -Recurse | ForEach-Object {
        [ordered]@{ path = $_.FullName.Substring($payload.Length + 1); sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
    })
    [ordered]@{ product = 'Glaze'; version = '0.1.0'; architecture = 'x64'; files = $files } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $payload 'release-files.json') -Encoding UTF8
    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    $zip = Join-Path $OutputDirectory ($name + '.zip')
    if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
    # .NET Framework's CreateFromDirectory writes backslash names under PowerShell 5.1.
    # Use standard ZIP separators so Explorer and other extractors agree on the layout.
    $archive = [IO.Compression.ZipFile]::Open($zip, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($file in Get-ChildItem -LiteralPath $stage -Recurse -File) {
            $relative = $file.FullName.Substring($stage.Length + 1).Replace('\', '/')
            [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, $relative,
                [IO.Compression.CompressionLevel]::Optimal) | Out-Null
        }
    } finally { $archive.Dispose() }
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
    "$hash  $name.zip" | Set-Content -LiteralPath ($zip + '.sha256') -Encoding ASCII
    Write-Host "$zip ($((Get-Item -LiteralPath $zip).Length) bytes), SHA256 $hash"
} finally {
    $stage = Assert-ChildPath $stage $OutputDirectory
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
}
