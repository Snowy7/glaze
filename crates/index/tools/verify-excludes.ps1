# Exercise default exclusions, marker transitions, and honest aggregates through the live pipe.
param([string]$Exe = 'D:\Coding\projects\desktop\wallpaper\target\release\mypaper-index.exe')
$ErrorActionPreference = 'Stop'
$base = Join-Path $env:USERPROFILE 'mypaper-index-fixtures'
$root = [IO.Path]::GetFullPath((Join-Path $base ('rules-' + [Guid]::NewGuid().ToString('N'))))
if (-not $root.StartsWith([IO.Path]::GetFullPath($base) + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid fixture root' }
function Remove-FixtureTree([string]$Relative) {
  $path = [IO.Path]::GetFullPath((Join-Path $root $Relative))
  if (-not $path.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid subtree cleanup path' }
  Remove-Item -LiteralPath $path -Recurse -Force
}
function Reply([string]$Command, [string]$Path) {
  $reply = & $Exe $Command $Path
  if ($LASTEXITCODE -ne 0) { throw 'Pipe request failed' }
  $reply | ConvertFrom-Json
}
function Await-Aggregate([string]$Path, [long]$Bytes, [bool]$Unknown) {
  $until = [DateTime]::UtcNow.AddSeconds(30)
  do {
    $answer = Reply '--sizes' $Path
    if ($answer.hits.Count -gt 0 -and $answer.hits[0].size -eq $Bytes -and $answer.hits[0].size_unknown -eq $Unknown) { return $answer }
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $until)
  throw "Aggregate did not converge: $Path expected bytes=$Bytes unknown=$Unknown; got $($answer | ConvertTo-Json -Depth 4 -Compress)"
}
function Put([string]$Relative, [int]$Bytes) {
  $path = Join-Path $root $Relative
  [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
  [IO.File]::WriteAllBytes($path, (New-Object byte[] $Bytes))
}
if ((& $Exe --status | ConvertFrom-Json).scanning) { throw 'Initial reconciliation is still running' }
try {
  Put 'plain.txt' 5
  $known = Await-Aggregate $root 5 $false
  Put 'node_modules\pkg\hidden.bin' 13
  $unknown = Await-Aggregate $root 5 $true
  $hidden = Reply '--query' ('path:"' + $root + '" hidden')
  if ($hidden.hits.Count -ne 0) { throw 'Excluded node_modules file leaked into search' }
  $stub = $unknown.hits | Where-Object name -eq 'node_modules'
  if (-not $stub.excluded -or -not $stub.size_unknown) { throw 'Missing excluded aggregate' }
  Remove-FixtureTree 'node_modules'
  $knownAgain = Await-Aggregate $root 5 $false
  Put 'rust\target\generated.bin' 17
  $beforeMarker = Await-Aggregate (Join-Path $root 'rust') 17 $false
  Put 'rust\target\CACHEDIR.TAG' 0
  $afterMarker = Await-Aggregate (Join-Path $root 'rust') 0 $true
  Remove-Item -LiteralPath (Join-Path $root 'rust\target\CACHEDIR.TAG')
  $afterMarkerRemoved = Await-Aggregate (Join-Path $root 'rust') 17 $false
  Put 'dotnet\bin\generated.bin' 19
  $beforeProject = Await-Aggregate (Join-Path $root 'dotnet') 19 $false
  Put 'dotnet\app.csproj' 0
  $afterProject = Await-Aggregate (Join-Path $root 'dotnet') 0 $true
  Remove-Item -LiteralPath (Join-Path $root 'dotnet\app.csproj')
  $afterProjectRemoved = Await-Aggregate (Join-Path $root 'dotnet') 19 $false
  Put 'dotnet\app.csproj' 0
  $withProjectAgain = Await-Aggregate (Join-Path $root 'dotnet') 0 $true
  Remove-FixtureTree 'dotnet'
  $afterDirectoryRemoved = Await-Aggregate $root 22 $false
  Put 'rust\target\CACHEDIR.TAG' 0
  $withRustMarkerAgain = Await-Aggregate (Join-Path $root 'rust') 0 $true
  Remove-FixtureTree 'rust\target'
  $afterRustDirectoryRemoved = Await-Aggregate $root 5 $false
  [pscustomobject]@{
    excluded_search_hits=$hidden.hits.Count
    excluded_stub=$stub
    known_before=$known.hits[0]
    unknown_with_exclusion=$unknown.hits[0]
    known_after_removal=$knownAgain.hits[0]
    rust_before=$beforeMarker.hits[0]
    rust_after_marker=$afterMarker.hits[0]
    rust_after_marker_removed=$afterMarkerRemoved.hits[0]
    dotnet_before=$beforeProject.hits[0]
    dotnet_after_project=$afterProject.hits[0]
    dotnet_after_project_removed=$afterProjectRemoved.hits[0]
    after_project_directory_removed=$afterDirectoryRemoved.hits[0]
    after_rust_build_directory_removed=$afterRustDirectoryRemoved.hits[0]
  } | ConvertTo-Json -Depth 5
} finally {
  if (-not $root.StartsWith([IO.Path]::GetFullPath($base) + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid fixture root' }
  if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}
