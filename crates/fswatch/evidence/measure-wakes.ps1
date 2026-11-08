param([string]$Label, [int]$Seconds = 30)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WatchThreadName {
    [DllImport("kernel32.dll")] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
    [DllImport("kernel32.dll")] static extern int GetThreadDescription(IntPtr thread, out IntPtr text);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    public static string Read(uint id) {
        var thread = OpenThread(0x0800, false, id);
        if (thread == IntPtr.Zero) return "";
        try {
            IntPtr text;
            if (GetThreadDescription(thread, out text) < 0) return "";
            try { return Marshal.PtrToStringUni(text); } finally { LocalFree(text); }
        } finally { CloseHandle(thread); }
    }
}
'@
function Sample {
    $rows = @{}
    foreach ($row in Get-CimInstance Win32_PerfRawData_PerfProc_Thread -Filter "Name LIKE 'Glaze%'") {
        $rows["$($row.IDProcess)/$($row.IDThread)"] = $row
    }
    return $rows
}
$started = (Get-Date).ToString('o')
$before = Sample
Start-Sleep -Seconds $Seconds
$after = Sample
$rows = foreach ($key in $after.Keys) {
    if (-not $before.ContainsKey($key)) { continue }
    $a = $before[$key]; $b = $after[$key]
    $elapsed = ([double]$b.Timestamp_Sys100NS - [double]$a.Timestamp_Sys100NS) / 10000000
    [pscustomobject]@{
        process = $b.Name.Split('/')[0]; pid = $b.IDProcess; tid = $b.IDThread
        thread = [WatchThreadName]::Read($b.IDThread)
        switches_per_sec = [Math]::Round(([double]$b.ContextSwitchesPersec - [double]$a.ContextSwitchesPersec) / $elapsed, 3)
        elapsed_sec = [Math]::Round($elapsed, 3)
    }
}
$output = Join-Path $PSScriptRoot "$Label-wakes.json"
@($rows | Sort-Object process, tid) | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $output -Encoding utf8
@{ started=$started; finished=(Get-Date).ToString('o'); requested_seconds=$Seconds; thread_count=@($rows).Count } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot "$Label-wakes-meta.json") -Encoding utf8
$rows | Where-Object { $_.thread -match 'watch|notify' -or $_.process -in 'glaze-core', 'glaze-index' } | Sort-Object process, switches_per_sec -Descending | Format-Table -AutoSize
Write-Output "Saved $output"
