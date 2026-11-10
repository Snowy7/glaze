// Disposable foreign process for the opt-in DWM integration test. Never activates.
using System;
using System.Drawing;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

class CornerProbeWindow : Form {
    [DllImport("dwmapi.dll")] static extern int DwmSetWindowAttribute(IntPtr h, int a, ref int v, int n);
    protected override bool ShowWithoutActivation { get { return true; } }
    public CornerProbeWindow() {
        Text = "Glaze owned Windows corner probe";
        StartPosition = FormStartPosition.Manual;
        Bounds = new Rectangle(120, 140, 680, 440);
        BackColor = Color.FromArgb(32, 32, 32);
        Controls.Add(new Label { Text = "Windows native frame · owned no-activate test process", ForeColor = Color.White, Dock = DockStyle.Fill, TextAlign = ContentAlignment.MiddleCenter });
    }
    protected override void WndProc(ref Message m) {
        if (m.Msg == 0x21) { m.Result = new IntPtr(3); return; } // MA_NOACTIVATE
        if (m.Msg == 0x8001) { int v = m.WParam.ToInt32(); DwmSetWindowAttribute(Handle, 33, ref v, 4); return; }
        if (m.Msg == 0x8002) { var next = new CornerProbeWindow(); next.Left += 720; next.Show(); return; }
        if (m.Msg == 0x8003) { ShowInTaskbar = m.WParam == IntPtr.Zero; FormBorderStyle = m.WParam == IntPtr.Zero ? FormBorderStyle.Sizable : FormBorderStyle.SizableToolWindow; return; }
        base.WndProc(ref m);
    }
    [STAThread] static void Main(string[] args) {
        Application.EnableVisualStyles();
        var form = new CornerProbeWindow();
        form.Shown += delegate { File.WriteAllText(args[0], System.Diagnostics.Process.GetCurrentProcess().Id.ToString()); };
        Application.Run(form);
    }
}
