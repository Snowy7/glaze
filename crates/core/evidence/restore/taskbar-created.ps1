# Posts TaskbarCreated to Glaze's own top-level windows only (never a broadcast), as Explorer
# does after it restarts, and prints which windows got it.
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class TC {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string w);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern uint RegisterWindowMessageW(string s);
  [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static List<string> Post() {
    uint msg = RegisterWindowMessageW("TaskbarCreated");
    var o = new List<string>();
    foreach (var cls in new[]{"glaze.bars.services","glaze.tray"}) {
      IntPtr h = IntPtr.Zero;
      while ((h = FindWindowExW(IntPtr.Zero, h, cls, null)) != IntPtr.Zero) {
        uint pid; GetWindowThreadProcessId(h, out pid);
        string name = System.Diagnostics.Process.GetProcessById((int)pid).ProcessName;
        if (!name.StartsWith("glaze-")) continue;
        o.Add(cls + " " + name + "#" + pid + " posted=" + PostMessageW(h, msg, IntPtr.Zero, IntPtr.Zero));
      }
    }
    return o;
  }
}
"@
[TC]::Post()
