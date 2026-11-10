param(
    [Parameter(Mandatory)][string]$Directory,
    [Parameter(Mandatory)][string]$Output,
    [string]$ReadObj = 'C:\Program Files\LLVM\bin\llvm-readobj.exe'
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $ReadObj)) {
    $ReadObj = (Get-Command llvm-readobj.exe -ErrorAction Stop).Source
}
$Directory = [IO.Path]::GetFullPath($Directory).TrimEnd('\')
$result = @(foreach ($file in Get-ChildItem -LiteralPath $Directory -File -Recurse | Where-Object { $_.Extension -in '.exe','.dll' }) {
    $report = & $ReadObj --file-headers --coff-imports $file.FullName
    if ($LASTEXITCODE -ne 0) { throw "Cannot inspect PE imports: $($file.Name)" }
    if (-not ($report | Select-String -SimpleMatch 'Machine: IMAGE_FILE_MACHINE_AMD64')) {
        throw "Not an x64 binary: $($file.FullName)"
    }
    $imports = @(foreach ($line in $report) {
        if ($line -match '^\s+Name: (.+\.dll)$') {
            $name = $Matches[1]
            $bundled = Test-Path -LiteralPath (Join-Path $file.DirectoryName $name)
            $system = $name -match '^(api-ms-|ext-ms-)' -or (Test-Path -LiteralPath (Join-Path $env:SystemRoot "System32\$name"))
            if ($name -match '^(vcruntime|msvcp|concrt|vcomp)\d' -and -not $bundled) {
                throw "CRT dependency must be app-local: $($file.Name) -> $name"
            }
            if (-not $bundled -and -not $system) { throw "Unresolved dependency: $($file.Name) -> $name" }
            [ordered]@{ dll = $name; resolution = $(if ($bundled) { 'bundled-app-local' } else { 'Windows-system' }) }
        }
    })
    [ordered]@{ file = $file.FullName.Substring($Directory.Length + 1); imports = $imports }
})
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Output -Encoding UTF8
Write-Host "Audited $($result.Count) x64 executables/DLLs; no missing static imports."
