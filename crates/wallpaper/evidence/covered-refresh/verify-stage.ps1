param([Parameter(Mandatory)][string]$Backup)
$ErrorActionPreference='Stop'
$lock=(Join-Path $env:LOCALAPPDATA 'Temp\glaze-tools\desktop.lock')
$config=Join-Path $env:APPDATA 'glaze\config.json'
$until=(Get-Date).AddMinutes(2)
while(-not (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)) {
  if((Get-Date) -ge $until){throw 'Desktop lock is busy'}
  Start-Sleep 2
}
try {
  Add-Type -AssemblyName UIAutomationClient
  Add-Type -AssemblyName UIAutomationTypes
  $studio=Get-Process glaze-studio
  $root=[System.Windows.Automation.AutomationElement]::FromHandle($studio.MainWindowHandle)
  $condition=New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,'Your desktop')
  $items=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)
  $actions=@(foreach($item in $items){
    $pattern=$null
    if($item.Current.ProcessId -eq $studio.Id -and $item.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$pattern}
  })
  if($actions.Count -ne 1){throw "Expected one own-process Your desktop action, found $($actions.Count)"}
  # Invoke a semantic action only: no mouse, keyboard, focus, activation or window placement.
  $actions[0].Toggle()
  Start-Sleep 1
  $original=[IO.File]::ReadAllText((Join-Path $backup 'config.json'))
  [IO.File]::WriteAllText($config,($original -replace '"theme": "rain-city-amber"','"theme": "aurora"'))
  Start-Sleep 4
  & powershell -NoProfile -File (Join-Path $PSScriptRoot 'capture-studio.ps1') -LockHeld -Out (Join-Path $PSScriptRoot 'studio-aurora.png')
} finally {
  try {
    Copy-Item -LiteralPath (Join-Path $backup 'config.json') -Destination $config -Force
    Start-Sleep 4
    Copy-Item -LiteralPath (Join-Path $backup 'wallpaper.json') -Destination (Join-Path $env:LOCALAPPDATA 'glaze\wallpaper.json') -Force
    & powershell -NoProfile -File (Join-Path $PSScriptRoot 'capture-studio.ps1') -LockHeld -Out (Join-Path $PSScriptRoot 'studio-restored.png')
  } finally {[IO.Directory]::Delete($lock)}
}
