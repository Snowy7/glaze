# Capture only to private storage. Inspect the redacted candidate before publishing it.
param(
    [Parameter(Mandatory=$true)][string]$Original,
    [Parameter(Mandatory=$true)][string]$Redacted,
    [string]$Metadata = ($Original + '.windows.json')
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
if(-not ('GlazeCapture' -as [type])) {
Add-Type -ReferencedAssemblies System.Drawing.dll,System.Windows.Forms.dll -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Runtime.InteropServices;
using System.Text;
using System.Windows.Forms;
public static class GlazeCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
    [StructLayout(LayoutKind.Sequential)] struct Point {public int X,Y;}
    public sealed class Window {
        public long Handle; public uint Pid; public int Z; public Rect Bounds;
        public bool Glaze,OpaquePanel,InvisibleAlpha,DesktopBackground; public Rect Client;
    }
    public sealed class Report {
        public Rect Desktop; public Window[] Before,After; public DateTime CapturedUtc;
        public string Policy="Grey external rectangles; only higher opaque Studio client areas occlude them. Transparent Glaze surfaces never exempt external content.";
    }
    delegate bool EnumProc(IntPtr hwnd,IntPtr state);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr state);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h,out Rect rect);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr h,out Rect rect);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h,ref Point point);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder text,int count);
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h,int attribute,out int value,int size);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h,int attribute,out Rect value,int size);
    [DllImport("user32.dll")] static extern bool GetLayeredWindowAttributes(IntPtr h,out uint key,out byte alpha,out uint flags);
    [DllImport("dwmapi.dll")] static extern int DwmFlush();
    static Rectangle R(Rect r){return Rectangle.FromLTRB(r.Left,r.Top,r.Right,r.Bottom);}
    static Rect Native(Rectangle r){return new Rect{Left=r.Left,Top=r.Top,Right=r.Right,Bottom=r.Bottom};}
    public static Window[] Windows(Rectangle desktop) {
        var result=new List<Window>();int z=0;
        if(!EnumWindows((h,state)=>{
            int order=z++;
            if(!IsWindowVisible(h)||IsIconic(h))return true;
            int cloaked;if(DwmGetWindowAttribute(h,14,out cloaked,4)==0 && cloaked!=0)return true;
            var cls=new StringBuilder(256);GetClassName(h,cls,cls.Capacity);
            Rect bounds;if(!GetWindowRect(h,out bounds))throw new Exception("Cannot locate a visible window; capture refused");
            Rect frame;if(DwmGetWindowAttribute(h,9,out frame,16)==0 && frame.Right>frame.Left && frame.Bottom>frame.Top)bounds=frame;
            if(!R(bounds).IntersectsWith(desktop))return true;
            uint pid;GetWindowThreadProcessId(h,out pid);
            string name="";try{using(var p=Process.GetProcessById((int)pid)){name=p.ProcessName;}}catch{}
            bool ours=name=="glaze-shell"||name=="glaze-studio"||name=="glaze-wallpaper"||name=="glaze-core";
            var window=new Window{Handle=h.ToInt64(),Pid=pid,Z=order,Bounds=bounds,Glaze=ours};
            // Record Explorer's desktop too, while keeping it distinct from app content.
            window.DesktopBackground=cls.ToString()=="Progman"||cls.ToString()=="WorkerW";
            uint key,flags;byte alpha;
            window.InvisibleAlpha=GetLayeredWindowAttributes(h,out key,out alpha,out flags) && (flags&2)!=0 && alpha==0;
            if(name=="glaze-studio" && cls.ToString()=="glaze.ui.app") {
                Rect client;var origin=new Point();
                if(GetClientRect(h,out client)&&ClientToScreen(h,ref origin)) {
                    window.OpaquePanel=true;
                    window.Client=new Rect{Left=origin.X,Top=origin.Y,Right=origin.X+client.Right,Bottom=origin.Y+client.Bottom};
                }
            }
            result.Add(window);return true;
        },IntPtr.Zero))throw new Exception("EnumWindows failed; capture refused");
        return result.ToArray();
    }
    static GraphicsPath Panel(Rect bounds) {
        var r=R(bounds);float d=Math.Min(28,Math.Min(r.Width,r.Height));
        var p=new GraphicsPath();
        p.AddArc(r.Left,r.Top,d,d,180,90);p.AddArc(r.Right-d,r.Top,d,d,270,90);
        p.AddArc(r.Right-d,r.Bottom-d,d,d,0,90);p.AddArc(r.Left,r.Bottom-d,d,d,90,90);p.CloseFigure();return p;
    }
    static void Redact(Graphics g,Window[] windows,Rectangle desktop) {
        foreach(var window in windows) {
            if(window.Glaze||window.InvisibleAlpha||window.DesktopBackground)continue;
            using(var region=new Region(Rectangle.Intersect(R(window.Bounds),desktop))) {
                foreach(var above in windows) {
                    if(above.Z>=window.Z)break;
                    if(above.OpaquePanel)using(var path=Panel(above.Client)){region.Exclude(path);}
                }
                g.FillRegion(Brushes.DimGray,region);
            }
        }
    }
    public static Report Capture(string original,string redacted) {
        SetProcessDpiAwarenessContext(new IntPtr(-4));DwmFlush();
        var desktop=SystemInformation.VirtualScreen;
        var before=Windows(desktop);
        using(var bitmap=new Bitmap(desktop.Width,desktop.Height)) {
            using(var g=Graphics.FromImage(bitmap)){g.CopyFromScreen(desktop.Left,desktop.Top,0,0,desktop.Size);}
            var after=Windows(desktop);
            bitmap.Save(original,System.Drawing.Imaging.ImageFormat.Png);
            using(var g=Graphics.FromImage(bitmap)) {
                g.TranslateTransform(-desktop.Left,-desktop.Top);
                g.SmoothingMode=SmoothingMode.None;
                Redact(g,before,desktop);Redact(g,after,desktop);
            }
            bitmap.Save(redacted,System.Drawing.Imaging.ImageFormat.Png);
            return new Report{Desktop=Native(desktop),Before=before,After=after,CapturedUtc=DateTime.UtcNow};
        }
    }
}
'@
}
foreach($path in @($Original,$Redacted,$Metadata)) {
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($path)))
}
$report=[GlazeCapture]::Capture($Original,$Redacted)
$report|ConvertTo-Json -Depth 8|Set-Content -LiteralPath $Metadata
@{candidate=$Redacted;metadata=$Metadata;externalWindows=@($report.Before|Where-Object {-not $_.Glaze}).Count}
