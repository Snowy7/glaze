# Run under the IMPLEMENTING.md desktop lock. Only this test AUMID/history is touched.
param(
    [ValidatePattern('^glaze\.NotificationTest\.[a-fA-F0-9]{32}$')]
    [Parameter(Mandatory)][string]$AppId,
    [switch]$Remove
)
$ErrorActionPreference = 'Stop'
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.UI.Notifications.ToastNotification, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
$registration = "HKCU:\Software\Classes\AppUserModelId\$AppId"
if ($Remove) {
    if (Test-Path -LiteralPath $registration) {
        try { [Windows.UI.Notifications.ToastNotificationManager]::History.Clear($AppId) }
        finally {
            # AppId is constrained to this script's test namespace and a single GUID path component.
            Remove-Item -LiteralPath $registration -Recurse -Force
        }
    }
    return
}
New-Item -Path $registration -Force | Out-Null
New-ItemProperty -LiteralPath $registration -Name DisplayName -Value 'Glaze notification test' -PropertyType String -Force | Out-Null
$document = New-Object Windows.Data.Xml.Dom.XmlDocument
$document.LoadXml('<toast><visual><binding template="ToastGeneric"><text>Glaze event test</text><text>A temporary toast for live delivery verification.</text></binding></visual></toast>')
$toast = [Windows.UI.Notifications.ToastNotification]::new($document)
$toast.Tag = 'live-test'
$toast.SuppressPopup = $true
$at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() * 1000
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($AppId).Show($toast)
@{ stage = 'toast_sent'; at_unix_us = $at; app_id = $AppId } | ConvertTo-Json -Compress
