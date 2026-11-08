# Round 5 Explorer proofs in order: control bursts (nothing attached), then the helper with the
# glass preset (this round's build): styled bursts, one screenshot, the 30-minute soak.
param([int]$Bursts = 3, [int]$SoakMinutes = 30, [switch]$SkipControl, [string]$StyledLog = "explorer-styled2.log")
$d = $PSScriptRoot
. "$d\lib.ps1"
$script:Log = "$d\run-explorer.log"
if (Get-Process glaze-styler -ErrorAction SilentlyContinue) { throw 'a styler helper is running' }
if (-not $SkipControl) { & "$d\explorer-bursts.ps1" -Log "$d\explorer-control.log" -Bursts $Bursts -Tag 'control (nothing attached)' }
$env:RUST_LOG = 'glaze_styler=debug,info'
$helperLog = Join-Path $env:LOCALAPPDATA 'glaze\glaze.log'
$mark = (Get-Item $helperLog).Length
$helper = Start-Process 'C:\glaze-targets\app-styler\release\glaze-styler.exe' -ArgumentList '--preview glass' -PassThru
Start-Sleep -Seconds 3
Note "helper $($helper.Id) started (--preview glass)"
& "$d\explorer-bursts.ps1" -Log "$d\$StyledLog" -Bursts $Bursts -Tag 'styled (glass)'
& "$d\shot-folder.ps1" -Out "$d\explorer-glass.png"
& "$d\soak.ps1" -Log "$d\soak.log" -Minutes $SoakMinutes
# The helper's log lines from this run.
$fs = [IO.File]::Open($helperLog, 'Open', 'Read', 'ReadWrite'); [void]$fs.Seek($mark, 'Begin')
$text = (New-Object IO.StreamReader $fs).ReadToEnd(); $fs.Close()
[IO.File]::WriteAllText("$d\helper.log", (($text -split "`n") -match 'glaze_styler' -join "`n"))
Stop-Process -Id $helper.Id -ErrorAction SilentlyContinue
Note "helper stopped; run done"
