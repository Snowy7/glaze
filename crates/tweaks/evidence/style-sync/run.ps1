# One bounded live case. Takes the shared desktop directory lock, applies a copy of the
# user's config, captures readbacks/pixels, then restores exact config bytes and registry
# baseline in finally. Run separate cases at least 30 seconds apart. No synthetic input.
param(
  [Parameter(Mandatory=$true)]
  [ValidateSet('liquid-dark','oneui-dark','fluent-dark','solid-dark','mono-dark','liquid-light','explicit','auto','palette','release')]
  [string]$Case,
  [string]$Probe = 'C:\mypaper-targets\style-sync\debug\examples\inspect_style_sync.exe'
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..\..'))
$out = $PSScriptRoot
$configPath = Join-Path $env:APPDATA 'mypaper\config.json'
$palettePath = Join-Path $env:LOCALAPPDATA 'mypaper\palette.json'
$journalPath = Join-Path $env:LOCALAPPDATA 'mypaper\undo.json'
$lock = 'C:\Users\you\AppData\Local\Temp\mypaper-tools\desktop.lock'
$tools = Split-Path $lock -Parent
if (-not (Test-Path -LiteralPath $Probe)) { throw "Build the read-only probe first: $Probe" }
Set-Location -LiteralPath $repo
if (-not (New-Item -ItemType Directory -Path $lock -ErrorAction SilentlyContinue)) { throw 'Desktop is in use; retry later' }

function Write-AtomicBytes([string]$Path, [byte[]]$Bytes) {
  $temporary = "$Path.style-sync.tmp"
  [IO.File]::WriteAllBytes($temporary, $Bytes)
  [IO.File]::Replace($temporary, $Path, [NullString]::Value)
}
function Write-Config($Config) {
  Write-AtomicBytes $configPath ([Text.Encoding]::UTF8.GetBytes(($Config | ConvertTo-Json -Depth 40)))
}
function Snapshot {
  $raw = & $Probe
  if ($LASTEXITCODE -ne 0) { throw 'Read-only probe failed' }
  return ($raw | ConvertFrom-Json)
}
function Check-Settled([string]$Label) {
  $deadline = (Get-Date).AddSeconds(18)
  do {
    $ErrorActionPreference = 'Continue'
    $raw = & $Probe --check 2>$null
    $ErrorActionPreference = 'Stop'
    if ($LASTEXITCODE -eq 0) {
      $raw | Set-Content -LiteralPath (Join-Path $out "$Label.json") -Encoding utf8
      return ($raw | ConvertFrom-Json)
    }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  $raw | Set-Content -LiteralPath (Join-Path $out "$Label-failed.json") -Encoding utf8
  throw 'Live registry or queryable DWM attributes did not settle'
}
function Capture([string]$Label) {
  foreach ($name in @('explorer', 'ApplicationFrameHost')) {
    if (Get-Process $name -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 }) {
      & powershell -NoProfile -File "$tools\winshot.ps1" -Process $name -Out (Join-Path $out "$Label-$name.png") | Out-Null
    }
  }
  & powershell -NoProfile -File "$tools\shot.ps1" -Out (Join-Path $out "$Label-desktop.png") | Out-Null
}
function Originals-Restored($Journal, $Current) {
  foreach ($entry in $Journal.entries) {
    $value = @($Current.registry | Where-Object { $_.key -eq $entry.key -and $_.name -eq $entry.name })[0].value
    $original = $entry.original
    if ($null -eq $original) { if ($null -ne $value) { return $false }; continue }
    if ($null -ne $original.dword) { $kind = 4; $bytes = [BitConverter]::GetBytes([uint32]$original.dword) }
    elseif ($null -ne $original.binary) { $kind = 3; $bytes = [byte[]]$original.binary }
    elseif ($null -ne $original.string) { $kind = 1; $bytes = [Text.Encoding]::Unicode.GetBytes($original.string + [char]0) }
    elseif ($null -ne $original.expandstring) { $kind = 2; $bytes = [Text.Encoding]::Unicode.GetBytes($original.expandstring + [char]0) }
    else { throw 'Unexpected journal type' }
    if ($null -eq $value -or $value.raw.kind -ne $kind -or ($value.raw.bytes -join ',') -cne ($bytes -join ',')) { return $false }
  }
  return $true
}
function Check-AppTokens($Resolved, [bool]$CheckDominant) {
  # The neighbouring app-styler soak already enabled glass; this probe never enables it.
  $path = Join-Path $env:LOCALAPPDATA 'mypaper\app-styler\settings.json'
  $deadline = (Get-Date).AddSeconds(12)
  do {
    $rules = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
    $accent = $rules.rules.'Rectangle#SelectionIndicator'.Fill
    $dominant = $rules.rules.'Grid#RootPageGrid'.Background
    $expectedDominant = '#80' + $Resolved.dominant.Substring(1)
    if ($accent -eq $Resolved.accent -and (-not $CheckDominant -or $dominant -eq $expectedDominant)) {
      @{accent=$accent;dominant_background=$dominant;dominant_checked=$CheckDominant;expected_accent=$Resolved.accent;expected_dominant_background=$expectedDominant} |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $out "$Case-app-tokens.json") -Encoding utf8
      return
    }
    Start-Sleep -Milliseconds 200
  } while ((Get-Date) -lt $deadline)
  throw "App-styler tokens did not follow the Windows resolver: accent=$accent, dominant=$dominant"
}

try {
  $configBytes = [IO.File]::ReadAllBytes($configPath)
  $paletteBytes = [IO.File]::ReadAllBytes($palettePath)
  $hashBefore = (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash
  $before = Snapshot
  $stylerBefore = @(Get-Process mypaper-styler -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
  $before.registry | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $out "$Case-before-registry.json") -Encoding utf8
} catch { [IO.Directory]::Delete($lock); throw }
$failure = $null
try {
  $variant = [Text.Encoding]::UTF8.GetString($configBytes) | ConvertFrom-Json
  $variant.look | Add-Member -NotePropertyName windows -NotePropertyValue ([pscustomobject]@{}) -Force
  $style = ($Case -split '-')[0]
  if ($style -notin @('liquid','oneui','fluent','solid','mono')) { $style = 'fluent' }
  if ($Case -eq 'auto') { $style = 'mono' }
  $appearance = if ($Case -eq 'liquid-light') { 'light' } else { 'dark' }
  $variant.look | Add-Member -NotePropertyName style -NotePropertyValue $style -Force
  $variant.look | Add-Member -NotePropertyName appearance -NotePropertyValue $appearance -Force
  if ($Case -eq 'explicit') {
    $variant.look.windows = [pscustomobject]@{
      accent='#c04080'; border='#44aa22'; caption='#e2e4ef'; caption_text='#132b44'
      corners='square'; backdrop='none'; apps='light'; accent_on_taskbar=$false; accent_on_title_bars=$true
    }
  }
  if ($Case -eq 'auto') { $variant.look.appearance = 'light' }
  $checkAppTokens = $variant.experimental.app_styler -and $variant.look.apps.preset -eq 'glass' -and $Case -in @('explicit','auto','palette')
  if ($Case -eq 'palette' -and $checkAppTokens) {
    $variant.look.apps | Add-Member -NotePropertyName settings -NotePropertyValue ([pscustomobject]@{
      rules=[pscustomobject]@{'Grid#RootPageGrid'=[pscustomobject]@{Background='$dominant/80'}}
    }) -Force
  }
  if ($Case -eq 'release') {
    $journal = Get-Content -LiteralPath $journalPath -Raw | ConvertFrom-Json
    $variant.look.PSObject.Properties.Remove('style')
    $variant.look.PSObject.Properties.Remove('appearance')
    $variant.look.PSObject.Properties.Remove('accent')
  }
  Write-Config $variant
  if ($Case -eq 'release') {
    $deadline = (Get-Date).AddSeconds(18)
    do {
      Start-Sleep -Milliseconds 200
      $during = Snapshot
    } while (((Test-Path -LiteralPath $journalPath) -or -not (Originals-Restored $journal $during)) -and (Get-Date) -lt $deadline)
    if ((Test-Path -LiteralPath $journalPath) -or -not (Originals-Restored $journal $during)) { throw 'Windows originals were not restored' }
    $during | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $out "$Case.json") -Encoding utf8
  } else {
    $during = Check-Settled $Case
    if ($Case -eq 'auto') {
      $variant.look.appearance = 'auto'
      Write-Config $variant
      $during = Check-Settled "$Case-return"
    }
    if ($Case -eq 'palette') {
      $changed = [Text.Encoding]::UTF8.GetString($paletteBytes) | ConvertFrom-Json
      $changed.accent = '#d06030'; $changed.dominant = '#223344'
      Write-AtomicBytes $palettePath ([Text.Encoding]::UTF8.GetBytes(($changed | ConvertTo-Json)))
      $during = Check-Settled "$Case-changed"
      if ($during.accent -ne '#d06030' -or $during.dominant -ne '#223344') { throw 'Palette event did not reach the shared tokens' }
    }
  }
  if ($checkAppTokens) { Check-AppTokens $during ($Case -eq 'palette') }
  Capture $Case
  if ($Case -notin @('release','auto','palette')) {
    & powershell -NoProfile -File (Join-Path $out 'frame.ps1') -ExpectedFile (Join-Path $out "$Case.json") -Out (Join-Path $out "$Case-frame.png")
    if ($LASTEXITCODE -ne 0) { throw 'Owned native title-bar paint check failed' }
  }
} catch { $failure = $_ }
finally {
  try {
    if ($Case -eq 'palette') { Write-AtomicBytes $palettePath $paletteBytes }
    Write-AtomicBytes $configPath $configBytes
    $deadline = (Get-Date).AddSeconds(18)
    do {
      Start-Sleep -Milliseconds 250
      $after = Snapshot
      $equal = ($before.registry | ConvertTo-Json -Depth 20 -Compress) -ceq ($after.registry | ConvertTo-Json -Depth 20 -Compress)
    } while (-not $equal -and (Get-Date) -lt $deadline)
    $after = Check-Settled "$Case-restored-look"
    $hashAfter = (Get-FileHash -LiteralPath $configPath -Algorithm SHA256).Hash
    $stylerAfter = @(Get-Process mypaper-styler -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
    $report = [ordered]@{
      case=$Case; config_sha256_before=$hashBefore; config_sha256_after=$hashAfter
      config_restored=($hashBefore -eq $hashAfter); all_eight_registry_values_restored=$equal
      palette_restored=([Convert]::ToBase64String([IO.File]::ReadAllBytes($palettePath)) -ceq [Convert]::ToBase64String($paletteBytes))
      app_styler_pids_before=$stylerBefore; app_styler_pids_after=$stylerAfter
      matching_windows=$after.matching_windows; failure=if ($failure) { $failure.ToString() } else { $null }
    }
    $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $out "$Case-restored.json") -Encoding utf8
    $report | ConvertTo-Json
    if (-not $equal -or $hashBefore -ne $hashAfter) { throw 'Restoration mismatch' }
  } finally { [IO.Directory]::Delete($lock) }
}
if ($failure) { throw $failure }
