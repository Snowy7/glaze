param(
    [Parameter(Mandatory)][string]$PackagePath,
    [string]$CertificateThumbprint,
    [string]$TimestampUrl
)
. "$PSScriptRoot\common.ps1"
$PackagePath = (Resolve-Path -LiteralPath $PackagePath).Path
$publisher = (Get-PackageManifest).Package.Identity.Publisher

if ($CertificateThumbprint) {
    $certificate = Get-Item -LiteralPath "Cert:\CurrentUser\My\$CertificateThumbprint"
} else {
    $certificate = Get-ChildItem Cert:\CurrentUser\My | Where-Object {
        $_.Subject -eq $publisher -and $_.FriendlyName -eq 'Glaze sparse package (development only)' -and
        $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date).AddDays(30)
    } | Sort-Object NotAfter -Descending | Select-Object -First 1
    if (-not $certificate) {
        $certificate = New-SelfSignedCertificate -Type Custom -Subject $publisher `
            -FriendlyName 'Glaze sparse package (development only)' -CertStoreLocation Cert:\CurrentUser\My `
            -KeyUsage DigitalSignature -KeyExportPolicy NonExportable -HashAlgorithm SHA256 `
            -NotAfter (Get-Date).AddYears(2) -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
    }
    # Always export the public leaf, including repeated builds with an existing trusted key.
    # The release installer needs it on another PC. No private key/PFX is exported.
    $public = Join-Path (Split-Path $PackagePath -Parent) 'glaze-development.cer'
    Export-Certificate -Cert $certificate -FilePath $public -Force | Out-Null
    # Trust just this public leaf for this user, never a root or the machine store.
    if (-not (Test-Path -LiteralPath "Cert:\CurrentUser\TrustedPeople\$($certificate.Thumbprint)")) {
        Import-Certificate -FilePath $public -CertStoreLocation Cert:\CurrentUser\TrustedPeople | Out-Null
    }
}
if ($certificate.Subject -ne $publisher -or -not $certificate.HasPrivateKey) {
    throw "Signing certificate must have a private key and Subject '$publisher'."
}
$arguments = @('sign', '/fd', 'SHA256', '/s', 'My', '/sha1', $certificate.Thumbprint)
if ($TimestampUrl) { $arguments += @('/tr', $TimestampUrl, '/td', 'SHA256') }
Invoke-PackageTool (Get-SdkTool 'signtool.exe') ($arguments + $PackagePath)
Write-Host "Signed with $($certificate.Thumbprint) (CurrentUser\My)."
