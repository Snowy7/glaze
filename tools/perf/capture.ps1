# Called by the native harness while it holds the desktop lock. Captures physical screen
# pixels without activating windows. No resizing: both 1440p monitors remain inspectable.
param([Parameter(Mandatory=$true)][string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System.Runtime.InteropServices;
public static class PerfDpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }
'@
[PerfDpi]::SetProcessDPIAware() | Out-Null
$rect = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = New-Object System.Drawing.Bitmap $rect.Width, $rect.Height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.CopyFromScreen($rect.X, $rect.Y, 0, 0, $bitmap.Size)
    $bitmap.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
} finally {
    $graphics.Dispose()
    $bitmap.Dispose()
}
