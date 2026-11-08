# Round 5: gallery-trials.ps1 under glass, attached with no rules, and nothing attached.
param([int]$N = 5, [string]$Trials = "gallery-trials.ps1", [string[]]$Variants = @("glass", "none", "control"), [string]$LogName = "gallery.log")
$d = $PSScriptRoot
$log = "$d\$LogName"
$styler = 'C:\glaze-targets\app-styler\release\glaze-styler.exe'
foreach ($variant in $Variants) {
  $helper = $null
  if ($variant -ne 'control') {
    $helper = Start-Process $styler -ArgumentList "--preview $variant" -PassThru
    Start-Sleep -Seconds 3
  }
  & "$d\$Trials" -Log $log -Tag $(if ($variant -eq 'none') { 'attached, no rules' } else { $variant }) -N $N
  if ($helper) { Stop-Process -Id $helper.Id -ErrorAction SilentlyContinue; Start-Sleep -Seconds 3 }
}
