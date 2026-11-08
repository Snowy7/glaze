# Paint check for the DWM colour attributes, which DwmGetWindowAttribute cannot read.
# Uses a temporary owned, nonactivating native frame; never writes another window.
param([Parameter(Mandatory=$true)][string]$ExpectedFile, [Parameter(Mandatory=$true)][string]$Out)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing @'
using System;
using System.Runtime.InteropServices;
using System.Windows.Forms;
public sealed class StyleSyncFrame : Form {
  protected override bool ShowWithoutActivation { get { return true; } }
  protected override CreateParams CreateParams {
    get { var p = base.CreateParams; p.ExStyle |= 0x08000000; return p; }
  }
}
public static class StyleSyncNative {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr dc, uint flags);
  [DllImport("dwmapi.dll")] public static extern int DwmSetWindowAttribute(IntPtr hwnd, uint attr, ref uint value, uint size);
}
'@
[StyleSyncNative]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null
$expected = Get-Content -Raw -LiteralPath $ExpectedFile | ConvertFrom-Json
$frame = New-Object StyleSyncFrame
$bitmap = $null; $graphics = $null
try {
  $frame.Text = "Glaze $($expected.style) / $($expected.appearance) - native DWM frame"
  $frame.ShowInTaskbar = $false
  $frame.StartPosition = 'Manual'
  $screen = [Windows.Forms.Screen]::AllScreens | Where-Object { -not $_.Primary } | Select-Object -First 1
  if (-not $screen) { $screen = [Windows.Forms.Screen]::PrimaryScreen }
  $frame.SetBounds($screen.WorkingArea.X + 90, $screen.WorkingArea.Y + 90, 620, 140)
  $frame.Show()
  $attributes = @{border=34;caption=35;text=36;corners=33;backdrop=38;dark=20}
  foreach ($property in $expected.expected_window_attributes.PSObject.Properties) {
    [uint32]$value = $property.Value
    $result = [StyleSyncNative]::DwmSetWindowAttribute($frame.Handle, $attributes[$property.Name], [ref]$value, 4)
    if ($result -ne 0) { throw "DWM rejected $($property.Name): $result" }
  }
  $deadline = (Get-Date).AddMilliseconds(500)
  do { [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 20 } while ((Get-Date) -lt $deadline)
  $bounds = New-Object StyleSyncNative+Rect
  [StyleSyncNative]::GetWindowRect($frame.Handle, [ref]$bounds) | Out-Null
  $bitmap = New-Object Drawing.Bitmap ($bounds.Right - $bounds.Left), ($bounds.Bottom - $bounds.Top)
  $graphics = [Drawing.Graphics]::FromImage($bitmap)
  $dc = $graphics.GetHdc()
  try { if (-not [StyleSyncNative]::PrintWindow($frame.Handle, $dc, 2)) { throw 'PrintWindow failed' } }
  finally { $graphics.ReleaseHdc($dc) }
  $strip = $bitmap.Clone([Drawing.Rectangle]::new(0, 0, $bitmap.Width, [Math]::Min(44, $bitmap.Height)), $bitmap.PixelFormat)
  try { $strip.Save($Out) } finally { $strip.Dispose() }
  [uint32]$caption = $expected.expected_window_attributes.caption
  $red = $caption -band 255; $green = ($caption -shr 8) -band 255; $blue = ($caption -shr 16) -band 255
  $matching = 0
  for ($y=8; $y -lt 34; $y++) {
    for ($x=35; $x -lt 470; $x++) {
      $pixel = $bitmap.GetPixel($x,$y)
      if ([Math]::Abs($pixel.R-$red) -le 2 -and [Math]::Abs($pixel.G-$green) -le 2 -and [Math]::Abs($pixel.B-$blue) -le 2) { $matching++ }
    }
  }
  $report = @{caption_pixels_matching=$matching;expected_rgb=@($red,$green,$blue);screenshot=[IO.Path]::GetFileName($Out)}
  $report | ConvertTo-Json | Set-Content -LiteralPath ([IO.Path]::ChangeExtension($Out, '.json')) -Encoding utf8
  if ($matching -lt 1000) { throw "Caption colour not visible: only $matching matching pixels" }
} finally {
  if ($graphics) { $graphics.Dispose() }; if ($bitmap) { $bitmap.Dispose() }
  $frame.Close(); $frame.Dispose()
}
