# Run after stopping the old desktop, before deploying Glaze. Originals are backed up;
# an existing Glaze profile is never overwritten. Re-running after migration is harmless.
param([switch]$ValidateOnly)
$ErrorActionPreference = 'Stop'
$profiles = @(
    @{ Root = $env:APPDATA; Old = 'mypaper'; New = 'glaze' },
    @{ Root = $env:LOCALAPPDATA; Old = 'mypaper'; New = 'glaze' },
    @{ Root = $env:LOCALAPPDATA; Old = 'dev.mypaper.studio'; New = 'dev.glaze.studio' }
)
foreach ($profile in $profiles) {
    $source = Join-Path $profile.Root $profile.Old
    $destination = Join-Path $profile.Root $profile.New
    if ((Test-Path -LiteralPath $source) -and (Test-Path -LiteralPath $destination)) {
        throw "Both profiles exist; reconcile them before migration: $source and $destination"
    }
    if (Test-Path -LiteralPath $source) { Write-Host "$source -> $destination" }
}
if ($ValidateOnly) { return }
if (Get-Process mypaper-*, glaze-* -ErrorAction SilentlyContinue) {
    throw 'Stop both desktops and Studio before migrating their data.'
}
$backup = Join-Path $env:LOCALAPPDATA ('glaze-migration-backups\' + (Get-Date -Format yyyyMMdd-HHmmss))
New-Item -ItemType Directory -Path $backup -Force | Out-Null
$replacements = @(
    @((Join-Path $env:APPDATA 'mypaper'), (Join-Path $env:APPDATA 'glaze')),
    @((Join-Path $env:LOCALAPPDATA 'mypaper'), (Join-Path $env:LOCALAPPDATA 'glaze'))
)
foreach ($profile in $profiles) {
    $source = Join-Path $profile.Root $profile.Old
    if (-not (Test-Path -LiteralPath $source)) { continue }
    $destination = Join-Path $profile.Root $profile.New
    $label = if ($profile.Root -eq $env:APPDATA) { 'roaming' } else { 'local-' + $profile.Old }
    Copy-Item -LiteralPath $source -Destination (Join-Path $backup $label) -Recurse
    # These paths are direct children of the explicitly named AppData roots.
    if ((Split-Path ([IO.Path]::GetFullPath($source)) -Parent) -ne $profile.Root -or
        (Split-Path ([IO.Path]::GetFullPath($destination)) -Parent) -ne $profile.Root) {
        throw 'Profile path escaped AppData'
    }
    Move-Item -LiteralPath $source -Destination $destination
    # Change only known app paths, executable names and our repository URL. User content,
    # plugin IDs and encrypted credentials retain their values. Preserve JSON formatting.
    foreach ($file in Get-ChildItem -LiteralPath $destination -Recurse -File -Filter '*.json') {
        $text = [IO.File]::ReadAllText($file.FullName)
        $updated = $text
        foreach ($pair in $replacements) {
            $updated = $updated.Replace($pair[0].Replace('\', '\\'), $pair[1].Replace('\', '\\'))
            $updated = $updated.Replace($pair[0].Replace('\', '/'), $pair[1].Replace('\', '/'))
        }
        $updated = $updated.Replace('mypaper-', 'glaze-').Replace('Snowy7/mypaper', 'Snowy7/glaze')
        if ($updated -ne $text) { [IO.File]::WriteAllText($file.FullName, $updated) }
    }
    $oldLog = Join-Path $destination 'mypaper.log'
    if (Test-Path -LiteralPath $oldLog) { Move-Item -LiteralPath $oldLog -Destination (Join-Path $destination 'glaze.log') }
}

# Copy generic integration credentials inside Windows. Never export or print secret blobs;
# retain the originals for rollback and preserve any credential already saved under Glaze.
Add-Type @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class GlazeCredentialMigration {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct Credential {
        public uint Flags, Type;
        public IntPtr TargetName, Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public uint CredentialBlobSize;
        public IntPtr CredentialBlob;
        public uint Persist, AttributeCount;
        public IntPtr Attributes, TargetAlias, UserName;
    }
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CredEnumerateW(string filter, uint flags, out uint count, out IntPtr credentials);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CredReadW(string target, uint type, uint flags, out IntPtr credential);
    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern bool CredWriteW(ref Credential credential, uint flags);
    [DllImport("advapi32.dll")] static extern void CredFree(IntPtr buffer);
    public static int Copy() {
        uint count; IntPtr entries;
        if (!CredEnumerateW("mypaper/*", 0, out count, out entries)) {
            int error = Marshal.GetLastWin32Error();
            if (error == 1168) return 0;
            throw new Win32Exception(error);
        }
        int copied = 0;
        try {
            for (int i = 0; i < count; i++) {
                var value = (Credential)Marshal.PtrToStructure(Marshal.ReadIntPtr(entries, i * IntPtr.Size), typeof(Credential));
                if (value.Type != 1) continue;
                string oldName = Marshal.PtrToStringUni(value.TargetName);
                string newName = "glaze/" + oldName.Substring("mypaper/".Length);
                IntPtr existing;
                if (CredReadW(newName, value.Type, 0, out existing)) { CredFree(existing); continue; }
                int error = Marshal.GetLastWin32Error();
                if (error != 1168) throw new Win32Exception(error);
                IntPtr name = Marshal.StringToHGlobalUni(newName);
                try {
                    value.TargetName = name;
                    if (!CredWriteW(ref value, 0)) throw new Win32Exception(Marshal.GetLastWin32Error());
                    copied++;
                } finally { Marshal.FreeHGlobal(name); }
            }
        } finally { CredFree(entries); }
        return copied;
    }
}
'@
Write-Host "Copied $([GlazeCredentialMigration]::Copy()) integration credentials inside Windows Credential Manager."
Write-Host "Original data backup: $backup"
