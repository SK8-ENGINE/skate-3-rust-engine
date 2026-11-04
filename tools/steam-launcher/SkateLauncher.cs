// Couch launcher for Steam / Steam Link (see README.md): a camcorder-shaped window (docs/hails-additions/41-...) with a
// menu for the controller, the keyboard, the mouse or Steam's desktop layout. launcher.ps1 does the work (versions, builds, launching, session checks) and
// this window shows its output; the entry and version lists come from launcher.ps1 too.
//   D-pad / left stick = move, A = select, B = back. Arrows / Enter / Esc / Space (Steam's desktop layout) work too.
//   SkateLauncher.exe <entry> [label] [@version]  runs an entry directly (one-click Steam shortcuts), shows the
//   result for a few seconds and closes. version-next / version-prev / version-set <id> / notes work the same way.
// Build (build.bat): C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe /nologo /target:winexe
//   /out:SkateLauncher.exe SkateLauncher.cs   (that compiler is C# 5: no string interpolation or ?.)
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using System.Windows.Forms;

static class Native {
    [StructLayout(LayoutKind.Sequential)]
    public struct XState { public uint Packet; public ushort Buttons; public byte LT, RT; public short LX, LY, RX, RY; }
    [DllImport("xinput1_4.dll")] public static extern int XInputGetState(int i, out XState s);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool AllowSetForegroundWindow(int pid);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool ReleaseCapture();
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, int msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
    [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern bool DeleteDC(IntPtr dc);
    [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc, IntPtr o);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr o);
    [StructLayout(LayoutKind.Sequential)] struct Pt { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct Sz { public int W, H; }
    [StructLayout(LayoutKind.Sequential, Pack = 1)] struct Blend { public byte Op, Flags, Alpha, Format; }
    [DllImport("user32.dll")] static extern bool UpdateLayeredWindow(IntPtr h, IntPtr dst, ref Pt pos, ref Sz size, IntPtr src, ref Pt srcPos, int key, ref Blend b, int flags);
    public static void DeleteHbitmap(IntPtr h) { DeleteObject(h); }
    // Hands a frame with per-pixel alpha to a layered window (the camcorder's outline is its own alpha).
    public static void Push(IntPtr hwnd, Bitmap bmp, int x, int y, byte alpha) {
        IntPtr screen = GetDC(IntPtr.Zero), mem = CreateCompatibleDC(screen), hb = bmp.GetHbitmap(Color.FromArgb(0)), old = SelectObject(mem, hb);
        var pos = new Pt { X = x, Y = y }; var size = new Sz { W = bmp.Width, H = bmp.Height }; var src = new Pt(); var bl = new Blend { Alpha = alpha, Format = 1 };
        UpdateLayeredWindow(hwnd, screen, ref pos, ref size, mem, ref src, 0, ref bl, 2);
        SelectObject(mem, old); DeleteObject(hb); DeleteDC(mem); ReleaseDC(IntPtr.Zero, screen);
    }

    // Windows only lets the foreground owner hand the foreground on; a window Steam or a hidden launcher starts is
    // otherwise left behind the last active one. Joining the foreground thread's input for the call lifts that.
    public static void Front(IntPtr h) {
        if (h == IntPtr.Zero) return;
        if (IsIconic(h)) ShowWindow(h, 9);
        uint fg = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero), me = GetCurrentThreadId();
        bool joined = fg != 0 && fg != me && AttachThreadInput(me, fg, true);
        BringWindowToTop(h); SetForegroundWindow(h);
        if (joined) AttachThreadInput(me, fg, false);
    }
}

// One menu row: greyed rows (On = false) can be selected to read why, but do nothing.
class Item {
    public string Text, Hint; public bool On = true; public Action Run;
    public string Info = ""; public string[] Records = new string[0];   // the play screen's detail panel
    public Item(string text, string hint, bool on, Action run) { Text = text; Hint = hint ?? ""; On = on; Run = run; }
}
class Page {
    public string Title; public List<Item> Items = new List<Item>(); public int Sel;
    public string Body;          // a text page (notes, results) instead of a list
    public Action OnBack;        // null = pop the page
    public string Mono;          // a fixed-width block under the body (a build's first error)
    public int Cols;             // > 0: the items are tiles like the main screen's, Cols wide (0 = full-width rows)
}

// Build progress from the build output (launcher.ps1 marks the build with LAUNCHER_BUILD lines and turns on cargo's own
// "Building [..] n/total" bar for the window). Every line also goes to the build log file it names.
class BuildView {
    static readonly System.Text.RegularExpressions.Regex Ansi = new System.Text.RegularExpressions.Regex(@"\x1B(\[[0-?]*[ -/]*[@-~]|\][^\x07\x1B]*(\x07|\x1B\\)|[@-Z\\-_])");
    static readonly System.Text.RegularExpressions.Regex Bar = new System.Text.RegularExpressions.Regex(@"Building \[[^\]]*\]\s*(\d+)/(\d+)(?::\s*(.*))?");
    static readonly System.Text.RegularExpressions.Regex Compiling = new System.Text.RegularExpressions.Regex(@"Compiling (\S+) v");
    static readonly System.Text.RegularExpressions.Regex ErrStart = new System.Text.RegularExpressions.Regex(@"^error(\[E\d+\])?: ");
    // Cargo waiting for another cargo (USER-ASKS 72: the laptop showed "0 crates compiled" while cargo printed "Blocking
    // waiting for file lock on build directory"); and its index / download stages, which come before any build step.
    static readonly System.Text.RegularExpressions.Regex Lock = new System.Text.RegularExpressions.Regex(@"Blocking waiting for file lock on (.+)$");
    static readonly System.Text.RegularExpressions.Regex Stage = new System.Text.RegularExpressions.Regex(@"^(Updating|Downloading|Downloaded|Locking|Fetching)\b");
    public string Waiting, WaitWho, StageText; public DateTime WaitSince;
    // The fetch before a build (LAUNCHER_WAIT lines, USER-ASKS 72): shown as WAITING once it has lasted 5 s.
    public string PreWait, LastPreWait; public DateTime PreWaitSince;
    public bool FetchWaiting() { return PreWait != null && (DateTime.Now - PreWaitSince).TotalSeconds >= 5; }
    public bool Active, Ended, Ok; public string Name = "", Commit = "", LogPath, Now = "";
    public int Done, Total, Compiled, Warnings; public DateTime Start, End;
    public readonly List<string> Error = new List<string>(); bool inError; string couldNot;
    public readonly List<string> Tail = new List<string>();   // the last clean lines, when there is no error block
    StreamWriter file;

    // The text after the last \r, no ANSI or control characters, tabs as spaces.
    public static string Clean(string s) {
        s = Ansi.Replace(s ?? "", "");
        int r = s.LastIndexOf('\r'); if (r >= 0) s = s.Substring(r + 1);
        var sb = new StringBuilder(s.Length);
        foreach (char c in s) { if (c == '\t') sb.Append("    "); else if (c >= ' ') sb.Append(c); }
        return sb.ToString().TrimEnd();
    }
    // True when the line is a marker or build output (kept out of the plain job log).
    public bool Feed(string raw) {
        if (raw.StartsWith("LAUNCHER_BUILD\tbegin\t")) {
            var f = raw.Split('\t');
            Close(); Active = true; Ended = false; Name = f.Length > 2 ? f[2] : ""; Commit = f.Length > 3 ? f[3] : ""; LogPath = f.Length > 4 ? f[4] : null;
            Done = Total = Compiled = Warnings = 0; Error.Clear(); Tail.Clear(); inError = false; couldNot = null; Now = ""; Start = DateTime.Now; Waiting = null; WaitWho = null; StageText = null;
            try { if (LogPath != null) { Directory.CreateDirectory(Path.GetDirectoryName(LogPath)); file = new StreamWriter(LogPath, false, new UTF8Encoding(false)); } } catch { file = null; }
            if (LastPreWait != null) { Note(LastPreWait); LastPreWait = null; }
            return true;
        }
        if (raw.StartsWith("LAUNCHER_SELF\t")) return true;   // the start check's own markers (b522)
        if (raw.StartsWith("LAUNCHER_WAIT\t")) {
            var f = raw.Split('\t');
            if (f.Length > 4 && f[1] == "begin") { PreWait = "fetching " + f[3] + " from " + f[4]; PreWaitSince = DateTime.Now; }
            else if (f.Length > 3 && f[1] == "end" && PreWait != null) { LastPreWait = (f[3] == "0" ? "fetched" : "fetch failed,") + PreWait.Substring(8) + " in " + Clock(DateTime.Now - PreWaitSince); PreWait = null; }
            return true;
        }
        // The log closes when the build ends, not when the job does: a game session follows a build in the same job, and
        // an open log blocked the after-session send (laptop 2026-10-19).
        if (raw.StartsWith("LAUNCHER_BUILD\tend\t")) { Active = false; Ended = true; End = DateTime.Now; Ok = raw.EndsWith("\t0"); Close(); return true; }
        if (!Active) return false;
        // launcher.ps1 names who holds the build folder (BUILDING-<id>.txt or a cargo process) before cargo blocks.
        if (raw.StartsWith("LAUNCHER_BUILD\twait\t")) {
            var f = raw.Split('\t'); WaitWho = f.Length > 3 ? f[3] : null; Note("holder: " + WaitWho);
            // "hold": launcher.ps1 itself waits (the version's exe is running), so the card shows it as a wait (b521 4c).
            if (f.Length > 4 && f[4] == "hold") { if (Waiting == null) { WaitSince = DateTime.Now; Note("waiting: " + WaitWho); } Waiting = WaitWho; }
            return true;
        }
        if (raw.StartsWith("LAUNCHER_BUILD\twaitend")) { EndWait(); return true; }
        string c = Clean(raw);
        var m = Bar.Match(c);
        if (m.Success) { Done = int.Parse(m.Groups[1].Value); Total = int.Parse(m.Groups[2].Value); if (m.Groups[3].Success) Now = m.Groups[3].Value.Trim(); }
        var cm = Compiling.Match(c);
        bool barOnly = m.Success && !cm.Success;
        if (file != null && !barOnly && c != "") { try { file.WriteLine(Ansi.Replace(raw, "").Replace("\r", "")); } catch { } }
        var lk = Lock.Match(c);
        if (lk.Success) {
            if (Waiting == null) { WaitSince = DateTime.Now; Note("waiting: " + lk.Groups[1].Value.Trim() + (WaitWho != null ? ", " + WaitWho : "")); }
            Waiting = "another build is using the " + lk.Groups[1].Value.Trim();
        }
        else if (m.Success || cm.Success) { EndWait(); StageText = null; }
        else if (Stage.IsMatch(c)) { EndWait(); StageText = c; }
        if (cm.Success) { Compiled++; Now = cm.Groups[1].Value; }
        if (inError) {
            if (c == "" || Error.Count >= 40) inError = false; else Error.Add(c);
        } else if (Error.Count == 0 && ErrStart.IsMatch(c)) {
            if (c.StartsWith("error: could not compile")) couldNot = c; else { inError = true; Error.Add(c); }
        } else if (c.StartsWith("warning: ") && !c.Contains(" generated ") && !c.StartsWith("warning: build failed")) Warnings++;
        if (c != "" && !barOnly) { Tail.Add(c); if (Tail.Count > 15) Tail.RemoveAt(0); }
        return true;
    }
    // The first error block, else cargo's summary and the last lines (e.g. Build.ps1's own failure).
    public List<string> FirstError() {
        if (Error.Count > 0) return Error;
        var l = new List<string>(); if (couldNot != null) l.Add(couldNot); l.AddRange(Tail); return l;
    }
    void EndWait() { if (Waiting != null) Note("waited " + Clock(DateTime.Now - WaitSince)); Waiting = null; }
    // A launcher line in the build log, so a session note shows when, why and how long a build waited.
    public void Note(string text) { if (file != null) { try { file.WriteLine("[launcher " + DateTime.Now.ToString("HH:mm:ss") + "] " + text); file.Flush(); } catch { } } }
    public static string Clock(TimeSpan t) { return ((int)t.TotalMinutes) + ":" + t.Seconds.ToString("00"); }
    public void Close() { if (file != null) { try { file.Dispose(); } catch { } file = null; } }
}

class LauncherForm : Form {    const ushort UP = 0x0001, DOWN = 0x0002, LEFT = 0x0004, RIGHT = 0x0008, A = 0x1000, B = 0x2000;
    int frame;
    static readonly string Here = AppDomain.CurrentDomain.BaseDirectory;
    static readonly string[] Labels = { "session", "ride", "grind", "bail", "push", "emit", "carve", "water", "marker" };
    static readonly string[] GameNames = { "skate3", "skate3rust", "rpcs3" };

    readonly Stack<Page> pages = new Stack<Page>();
    readonly Timer tick = new Timer();
    readonly List<string> log = new List<string>();
    readonly BuildView build = new BuildView(); bool dirty;
    readonly string[] direct; readonly bool afterUpdate;
    Dictionary<string, string> status = new Dictionary<string, string>();
    Process job; string jobTitle; bool jobHidGame; IntPtr gameWindow; DateTime hidAt; int closeAt = -1; DateTime closeTime;
    string waitEntry, waitLabel, waitName;   // waiting for another game to close before starting
    bool cancelArmed, cancelled, cancelledInFetch;  // B pressed once / twice while a build or its fetch waits
    ushort prevPad; int held; string lastAction = ""; bool lastFromPad; DateTime lastActionTime = DateTime.MinValue;
    Font monoFont, titleFont, itemFont, smallFont, heroFont, markFont, markLight, playFont, iconFont, iconSmall, itemBold, smallBold, bodyFont, splashMark, splashLight;

    public LauncherForm(string[] argv) {
        // --after-update: started by the previous exe after it updated itself (shows what changed), not a direct entry.
        afterUpdate = Array.IndexOf(argv, "--after-update") >= 0;
        sameMonitorFlag = Array.IndexOf(argv, "--same-monitor") >= 0;
        direct = Array.FindAll(argv, a => a != "--after-update" && a != "--same-monitor");
        Text = "Skate launcher";
        // The LCD panel as its own window (no frame, its outline is the drawing's alpha): 62 % of the work area's height
        // at the panel's shape, centred. On the main monitor, or with --same-monitor / the Tools setting on the monitor
        // of the window it was started from.
        var area = StartScreen().WorkingArea;
        int h = Math.Min(area.Height * 62 / 100, area.Width * 62 / 100 * 760 / 1240);
        FormBorderStyle = FormBorderStyle.None;
        StartPosition = FormStartPosition.Manual; ClientSize = new Size(h * 1240 / 760, h);
        Location = new Point(area.Left + (area.Width - Width) / 2, area.Top + (area.Height - Height) / 2);
        RollMarks(new Random());
        BackColor = Color.FromArgb(18, 20, 24); ForeColor = Color.White;
        DoubleBuffered = true; KeyPreview = true;
        try { Icon = Icon.ExtractAssociatedIcon(Application.ExecutablePath); } catch { }
        tick.Interval = 16; tick.Tick += delegate { Poll(); if (needPresent) Present(); };
        // The window paints first; the start check (launcher.ps1 pending: self update, session check) runs behind a card
        // (it froze the start with no window during a slow self-update fetch; b522).
        Load += delegate { tick.Start(); };
        Shown += delegate { Start(); };
        Resize += delegate { MakeFonts(); Invalidate(); };
        MakeFonts();
    }

    void MakeFonts() { MakeFonts(ClientSize.Height); }
    void MakeFonts(int height) {
        float h = Math.Max(480, height);
        titleFont = new Font("Segoe UI Semibold", h / 24f, GraphicsUnit.Pixel);
        heroFont = new Font("Segoe UI Semibold", h / 17f, GraphicsUnit.Pixel);
        itemFont = new Font("Segoe UI", h / 34f, GraphicsUnit.Pixel);
        itemBold = new Font("Segoe UI Semibold", h / 34f, GraphicsUnit.Pixel);
        smallFont = new Font("Segoe UI", h / 52f, GraphicsUnit.Pixel);
        monoFont = new Font("Consolas", h / 50f, GraphicsUnit.Pixel);
        smallBold = new Font("Segoe UI Semibold", h / 52f, GraphicsUnit.Pixel);
        bodyFont = new Font("Segoe UI", h / 42f, GraphicsUnit.Pixel);
        markFont = new Font("Segoe UI Black", h / 30f, GraphicsUnit.Pixel);
        markLight = new Font("Segoe UI Light", h / 30f, GraphicsUnit.Pixel);
        splashMark = new Font("Segoe UI Black", h / 11f, GraphicsUnit.Pixel);
        splashLight = new Font("Segoe UI Light", h / 11f, GraphicsUnit.Pixel);
        playFont = new Font("Segoe UI Black", h / 26f, GraphicsUnit.Pixel);
        iconFont = new Font("Segoe MDL2 Assets", h / 26f, GraphicsUnit.Pixel);
        iconSmall = new Font("Segoe MDL2 Assets", h / 40f, GraphicsUnit.Pixel);
    }

    public string WearTest(string png) {
        snapshotMode = true; snapPanel = 1; fixedT = 5; snapSeed = new Random().Next(); RefreshStatus(); pages.Push(MainPage());
        int w = 1243, h = 699;
        Func<Bitmap> frame = delegate {
            var bmp = new Bitmap(w, h, PixelFormat.Format32bppPArgb);
            using (var g = Graphics.FromImage(bmp)) { g.Clear(Color.Transparent); flipScale = -1; Render(g, w, h); }
            return bmp;
        };
        var keep = new List<Mark>(marks);
        frame().Dispose();   // the first frame builds the caches (fonts, scene, flip faces); compare later ones only
        int jitter = 0;
        string jitterAt = "";
        using (var x1 = frame()) using (var x2 = frame()) { int[] p1 = Pixels(x1), p2 = Pixels(x2); for (int i = 0; i < p1.Length; i++) if (p1[i] != p2[i]) { if (jitter < 3) jitterAt += string.Format(" ({0},{1}) {2:X8}/{3:X8}", i % w, i / w, p1[i], p2[i]); jitter++; } }
        notFit = 0;
        using (var with = frame()) {
            int unfit = notFit;
            marks.Clear(); var without = frame(); marks.AddRange(keep);
            marksUnclipped = true; var loose = frame(); marksUnclipped = false;
            using (var mask = new Bitmap(w, h, PixelFormat.Format32bppPArgb)) {
                using (var g = Graphics.FromImage(mask)) {
                    g.TranslateTransform(devX, devY); g.ScaleTransform(devS, devS);
                    using (var b = new SolidBrush(Color.White)) using (var o = WearMap()) g.FillRegion(b, o);
                }
                int[] a = Pixels(with), n = Pixels(without), l = Pixels(loose), mk = Pixels(mask);
                int changed = 0, stray = 0, spill = 0;
                var where = RectangleF.Empty;
                for (int i = 0; i < a.Length; i++) {
                    bool bad = false;
                    if (a[i] != n[i]) { changed++; if ((mk[i] & 0xFFFFFF) != 0xFFFFFF) { stray++; bad = true; } }
                    if (l[i] != a[i]) { spill++; bad = true; }
                    if (bad) { var px = new RectangleF(i % w, i / w, 1, 1); where = where.IsEmpty ? px : RectangleF.Union(where, px); }
                }
                if (!where.IsEmpty) {
                    // Where it went wrong: red = a mark off the camera or on the glass, yellow = a tag or doodle over its edge.
                    var bad = new int[a.Length];
                    for (int i = 0; i < a.Length; i++) bad[i] = l[i] != a[i] ? unchecked((int)0xFFFFD000) : (a[i] != n[i] && (mk[i] & 0xFFFFFF) != 0xFFFFFF) ? unchecked((int)0xFFFF2020) : unchecked((int)0x30000000);
                    using (var bb = new Bitmap(w, h, PixelFormat.Format32bppPArgb)) {
                        var d = bb.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.WriteOnly, PixelFormat.Format32bppPArgb);
                        System.Runtime.InteropServices.Marshal.Copy(bad, 0, d.Scan0, bad.Length); bb.UnlockBits(d);
                        bb.Save(Path.ChangeExtension(png, ".bad.png"), ImageFormat.Png);
                    }
                }
                without.Dispose(); loose.Dispose();
                with.Save(png, ImageFormat.Png);
                var kinds = new int[5]; foreach (var m in keep) kinds[m.Kind]++;
                string tags = string.Join(", ", keep.FindAll(m => m.Kind == 0).ConvertAll(m => "\"" + m.Text + "\" on " + new[] { "bottom bezel", "left strip" }[m.Spot]).ToArray());
                return (jitter > 0 ? "UNSTABLE (the same frame twice differs by " + jitter + " px:" + jitterAt + ")  " : "") + string.Format("{0}  changed {1} px, off the wear map (camera UI, glass, outside) {2} px, tags / doodles spilling over {3} px, not clear of the sticker " + unfit + "  ({4} tags, {5} doodles, {6} scratches, {7} worn edges, {8} tape)  {9}",
                    stray == 0 && spill == 0 && unfit == 0 && changed > 0 && jitter == 0 ? "PASS" : "FAIL", changed, stray, spill, kinds[0], kinds[1], kinds[2], kinds[3], kinds[4], tags) + (where.IsEmpty ? "" : "  bad pixels in " + where);
            }
        }
    }
    static int[] Pixels(Bitmap b) {
        var data = b.LockBits(new Rectangle(0, 0, b.Width, b.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppPArgb);
        var px = new int[b.Width * b.Height];
        System.Runtime.InteropServices.Marshal.Copy(data.Scan0, px, 0, px.Length);
        b.UnlockBits(data);
        return px;
    }

    public string Bench(int w, int h) {
        snapshotMode = true; RefreshStatus(); pages.Push(MainPage());
        var sb = new StringBuilder();
        using (var bmp = new Bitmap(w, h, PixelFormat.Format32bppPArgb)) using (var g = Graphics.FromImage(bmp)) {
            foreach (float at in new[] { 0.3f, 0.5f, 0.65f, 0.9f, 1f }) {
                snapPanel = at; flipScale = -1; Render(g, w, h);
                var sw = Stopwatch.StartNew();
                for (int i = 0; i < 20; i++) { g.Clear(Color.Transparent); Render(g, w, h); }
                double ms = sw.Elapsed.TotalMilliseconds / 20;
                sw.Restart();
                for (int i = 0; i < 20; i++) { IntPtr hb = bmp.GetHbitmap(Color.FromArgb(0)); Native.DeleteHbitmap(hb); }
                sb.AppendLine(string.Format(System.Globalization.CultureInfo.InvariantCulture, "assembly {0:0.00}: render {1:0.0} ms, GetHbitmap {2:0.0} ms", at, ms, sw.Elapsed.TotalMilliseconds / 20));
            }
        }
        return sb.ToString();
    }

    // Draws one screen into a PNG (the --snapshot mode): main, versions, tools or job (a sample build log).
    public void Snapshot(string file, string screen, int w, int h) {
        // "<screen>#7": the sticker from seed 7; "<screen>@0.4": 40 % through the power-on (the camera unfolding).
        snapshotMode = true; snapPanel = 1;
        int hash = screen.IndexOf('#');
        if (hash > 0) { int.TryParse(screen.Substring(hash + 1), out snapSeed); screen = screen.Substring(0, hash); }
        RollMarks(new Random(snapSeed));
        int at = screen.IndexOf('@');
        if (at > 0) { float.TryParse(screen.Substring(at + 1), System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out snapPanel); screen = screen.Substring(0, at); }
        MakeFonts(h);
        RefreshStatus();
        if (screen == "checking") { starting = true; selfFetching = true; startSince = DateTime.Now.AddSeconds(-7); }
        else pages.Push(MainPage());
        if (screen == "versions") pages.Push(VersionPage());
        if (screen == "tools") pages.Push(EntryPage("Tools", "tools"));
        if (screen == "setup") pages.Push(WizardPage());
        if (screen == "play" || screen == "play-traceall") pages.Push(EntryPage("Play: " + St("version"), "rust"));
        if (screen == "play-traceall") pages.Peek().Sel = Math.Min(1, pages.Peek().Items.Count - 1);
        if (screen == "waiting") {
            job = new Process(); jobTitle = "Update and build";
            build.Feed("LAUNCHER_BUILD\tbegin\thails-additions\t94774ce\t");
            build.Start = DateTime.Now.AddSeconds(-75);
            build.Feed("LAUNCHER_BUILD\twait\tbuild\thails-additions is being built by the shell since 16:41 (pid 21412)");
            build.Feed("    Blocking waiting for file lock on build directory");
            build.WaitSince = DateTime.Now.AddSeconds(-72);
        }
        if (screen == "fetching") {
            job = new Process(); jobTitle = "Update and build"; jobStart = DateTime.Now.AddSeconds(-44);
            log.Add("  Checking hails-additions on origin...");
            build.Feed("LAUNCHER_WAIT\tbegin\tfetch\thails-additions\torigin");
            build.PreWaitSince = DateTime.Now.AddSeconds(-42);
        }
        if (screen == "build" || screen == "failed") {
            job = new Process(); jobTitle = "Update and build";
            build.Feed("LAUNCHER_BUILD\tbegin\thails-additions\t94774ce\t");
            build.Start = DateTime.Now.AddSeconds(-190);
            foreach (var l in new[] { "   Compiling skate-core v0.1.0", "warning: unused import: `skitch_enabled`", " --> crates\\skate-game\\src\\physics.rs:81:53", "", "    Building [=======>        ] 312/540: skate-game, bevy_pbr" }) build.Feed(l);
            if (screen == "failed") foreach (var l in new[] { "error[E0308]: mismatched types", "  --> crates\\skate-game\\src\\physics\\grab_view.rs:42:5", "   |", "42 |     let x: u32 = 1.0;", "   |            ---   ^^^ expected `u32`, found floating-point number", "" }) build.Feed(l);
        }
        if (screen == "job") {
            job = new Process(); jobTitle = "Building hails-additions"; jobStart = DateTime.Now.AddSeconds(-74);
            foreach (var l in new[] { "Fetching hails-additions at 94774ce", "   Compiling bevy_render v0.18.1", "   Compiling skate-core v0.1.0", "warning: unused import: `skitch_enabled`", "   Compiling skate-game v0.1.0" }) log.Add(l);
        }
        using (var bmp = new Bitmap(w, h)) {
            using (var g = Graphics.FromImage(bmp)) { Render(g, w, h); }

            bmp.Save(file, System.Drawing.Imaging.ImageFormat.Png);
        }
    }

    // ---------- launcher.ps1 ----------
    static ProcessStartInfo Ps(string args) {
        var psi = new ProcessStartInfo("powershell.exe", "-NoProfile -ExecutionPolicy Bypass -File \"" + Path.Combine(Here, "launcher.ps1") + "\" " + args) {
            UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = Here,
            RedirectStandardOutput = true, RedirectStandardError = true, RedirectStandardInput = true };
        // The window's build view: launcher.ps1 marks the build and turns on cargo's progress bar.
        psi.EnvironmentVariables["SKATE_LAUNCHER_UI"] = "2";
        var oem = Encoding.GetEncoding(System.Globalization.CultureInfo.CurrentCulture.TextInfo.OEMCodePage);
        psi.StandardOutputEncoding = oem; psi.StandardErrorEncoding = oem;
        return psi;
    }
    // Runs a quick command and returns its output (the lists, notes, status).
    static string Run(string args, out int code) {
        using (var p = Process.Start(Ps(args))) {
            p.StandardInput.Close();
            string o = p.StandardOutput.ReadToEnd(); p.WaitForExit(); code = p.ExitCode;
            return o.TrimEnd('\r', '\n');   // not tabs: a last row can end in an empty column
        }
    }
    static string Run(string args) { int c; return Run(args, out c); }
    // The lists behind the main tiles (Versions, What to test, Recomp, RPCS3, Tools): each launcher.ps1 call is a new
    // powershell.exe (1 to 2.5 s on the laptop, 2026-10-19), so they are fetched in the background at start and after
    // every change (version picked, job finished) and the tiles open from memory. A tile pressed before its fetch is
    // done waits only for the rest of it.
    static readonly string[] Prefetched = { "entries", "versions", "notes", "status" };
    static readonly Dictionary<string, Task<string>> cache = new Dictionary<string, Task<string>>();
    static Task<string> Fetch(string args) {
        lock (cache) {
            Task<string> t;
            if (!cache.TryGetValue(args, out t)) { t = Task.Run(() => Run(args)); cache[args] = t; }
            return t;
        }
    }
    static string Listed(string args) {
        if (Array.IndexOf(Prefetched, args) < 0) return Run(args);
        var t = Fetch(args);
        try { return t.GetAwaiter().GetResult(); }
        catch { lock (cache) { Task<string> c; if (cache.TryGetValue(args, out c) && c == t) cache.Remove(args); } throw; }
    }
    static void Prefetch() { foreach (var a in Prefetched) Fetch(a); }
    // Waits for the prefetched lists off the window thread (a failed one is fetched again when read), so the menu is
    // built from memory: waiting on them in the window froze it for seconds, and Windows offered to end it (USER-ASKS 436).
    static void WaitListed() {
        var all = new List<Task>();
        foreach (var a in Prefetched) all.Add(Fetch(a));
        try { Task.WaitAll(all.ToArray()); } catch { }
    }
    void WhenListed(Action then) { Task.Run((Action)WaitListed).ContinueWith(t => BeginInvoke(then)); }
    static void ForgetCache() { lock (cache) cache.Clear(); }
    static List<string[]> Rows(string args, int min) {
        var rows = new List<string[]>();
        foreach (var l in Listed(args).Split('\n')) { var f = l.TrimEnd('\r').Split('\t'); if (f.Length >= min) rows.Add(f); }
        return rows;
    }
    static string StateDir() {
        string here = Path.GetFullPath(Here).TrimEnd('\\');
        string repo = Path.GetFullPath(Path.Combine(here, "..", ".."));
        if (here.StartsWith(Path.Combine(repo, "tools") + "\\", StringComparison.OrdinalIgnoreCase)) return Path.Combine(repo, ".local", "steam-launcher");
        return here;
    }
    bool sameMonitorFlag;
    static string SameMonitorFile() { return Path.Combine(StateDir(), "open-on-launching-monitor.txt"); }
    static bool SameMonitorSetting() { return File.Exists(SameMonitorFile()); }
    Screen StartScreen() {
        if (!sameMonitorFlag && !SameMonitorSetting()) return Screen.PrimaryScreen;
        IntPtr fg = Native.GetForegroundWindow();
        return fg != IntPtr.Zero ? Screen.FromHandle(fg) : Screen.FromPoint(Cursor.Position);
    }
    Item SameMonitorItem() {
        Func<string> text = () => "Open on: " + (SameMonitorSetting() ? "the monitor it was started from" : "the main monitor");
        Item item = null;
        item = new Item(text(), "Where the launcher window opens. --same-monitor on the command line does the same for one start.", true, delegate {
            try { if (SameMonitorSetting()) File.Delete(SameMonitorFile()); else File.WriteAllText(SameMonitorFile(), "on\n"); } catch { }
            item.Text = text();
        });
        return item;
    }
    static string LastSession() { string f = Path.Combine(StateDir(), "last_session.txt"); return File.Exists(f) ? File.ReadAllText(f).Trim() : "No session yet."; }
    static string VerArg(string v) { return string.IsNullOrEmpty(v) ? "" : " -Version " + v; }
    static string RunningGames() {
        string r = "";
        foreach (var n in GameNames) {
            var found = Process.GetProcessesByName(n);
            if (found.Length > 0) r += (r == "" ? "" : ", ") + n + ".exe";
            foreach (var p in found) p.Dispose();
        }
        // A Refresh Game Files run (launcher.ps1 Refresh-GameFiles) is named instead of its exe (USER-ASKS 72).
        string marker = Path.Combine(StateDir(), "REFRESHING.txt");
        if (r != "" && File.Exists(marker)) { try { r = "Refresh game files (" + File.ReadAllText(marker).Trim() + ")"; } catch { } }
        return r;
    }
    // From the prefetched status when `cached` (just fetched with the lists), else a fresh run (after a share toggle).
    void RefreshStatus() { RefreshStatus(false); }
    void RefreshStatus(bool cached) {
        if (!cached) lock (cache) cache.Remove("status");
        status.Clear();
        foreach (var r in Rows("status", 2)) status[r[0]] = r[1];
    }
    string St(string k) { string v; return status.TryGetValue(k, out v) ? v : ""; }

    // ---------- start ----------
    // The launcher updated its own exe (launcher.ps1 wrote restart.txt): start the new one and wait for it hidden, so
    // Steam keeps the process it started (its controller layout and "playing" state) until the new window closes.
    bool RestartIfUpdated(bool showLast) {
        string f = Path.Combine(StateDir(), "restart.txt");
        if (!File.Exists(f)) return false;
        try { File.Delete(f); } catch { }
        string exe = Path.Combine(Here, "SkateLauncher.exe");
        if (!File.Exists(exe)) return false;
        var args = new List<string>(direct);
        if (showLast) args.Add("--after-update");
        if (sameMonitorFlag) args.Add("--same-monitor");
        Process child;
        SkateLauncher.ReleaseInstance();
        try { child = Process.Start(new ProcessStartInfo(exe, string.Join(" ", args.ConvertAll(a => "\"" + a + "\"").ToArray())) { UseShellExecute = false }); } catch { return false; }
        tick.Stop(); Hide();
        child.EnableRaisingEvents = true;
        child.Exited += delegate { BeginInvoke((Action)Close); };
        if (child.HasExited) Close();
        return true;
    }
    bool starting, skipArmed; DateTime startSince;
    // The start check's process, whether its self-update fetch is done (LAUNCHER_SELF) and whether B skipped it.
    readonly object startLock = new object(); Process startProc; volatile bool selfFetching, selfFetched, skipped;
    // The start check's current step (LAUNCHER_STAGE lines), shown under the rolling board.
    volatile string startStage;
    const double SplashMinSeconds = 1.1;
    void Start() {
        Activate(); Native.Front(Handle);
        starting = true; startSince = DateTime.Now; selfFetching = selfFetched = skipped = skipArmed = false; startStage = null; Invalidate();
        // The splash stays at least SplashMinSeconds, so a fast check does not flash it.
        Task.Run(() => {
            int code = RunPending(false);
            // The menu's lists load behind the splash (not when an updated exe takes over, or in direct mode).
            if (direct.Length == 0 && !File.Exists(Path.Combine(StateDir(), "restart.txt"))) { startStage = "Loading the menu"; Prefetch(); WaitListed(); }
            int left = (int)((SplashMinSeconds - (DateTime.Now - startSince).TotalSeconds) * 1000);
            if (left > 0) Task.Delay(left).Wait();
            return code;
        }).ContinueWith(t => BeginInvoke((Action)(() => FinishStart(t.IsFaulted ? 0 : t.Result))));
    }
    // launcher.ps1 pending; killed by B during the fetch, it runs again without the self update so the rest still happens.
    int RunPending(bool noSelf) {
        var psi = Ps("pending");
        if (noSelf) psi.EnvironmentVariables["SKATE_LAUNCHER_NO_SELF"] = "1";
        int code;
        using (var p = Process.Start(psi)) {
            lock (startLock) startProc = p;
            p.StandardInput.Close();
            string line;
            while ((line = p.StandardOutput.ReadLine()) != null) {
                if (line.StartsWith("LAUNCHER_SELF\tfetching")) selfFetching = true;
                else if (line.StartsWith("LAUNCHER_SELF\tfetched")) selfFetched = true;
                else if (line.StartsWith("LAUNCHER_STAGE\t")) startStage = line.Substring(15);
            }
            p.WaitForExit(); code = p.ExitCode;
            lock (startLock) startProc = null;
        }
        return skipped && !noSelf ? RunPending(true) : code;
    }
    // B during the self-update fetch: the first press arms, the second kills the check (never after the fetch, when files copy).
    bool CanSkip() { return starting && selfFetching && !selfFetched && !skipped && (DateTime.Now - startSince).TotalSeconds >= 3; }
    void SkipPress() {
        if (!CanSkip()) return;
        if (!skipArmed) { skipArmed = true; dirty = true; return; }
        lock (startLock) {
            if (startProc == null || selfFetched) return;
            skipped = true;
            try { Process.Start(new ProcessStartInfo("taskkill.exe", "/T /F /PID " + startProc.Id) { CreateNoWindow = true, UseShellExecute = false }); } catch { }
        }
    }
    void FinishStart(int code) {
        starting = false;
        if (RestartIfUpdated(code == 10)) return;
        string ent = "", lbl = "", ver = "";
        foreach (var a in direct) { if (a.StartsWith("@")) ver = a.Substring(1); else if (ent == "") ent = a; else if (lbl == "") lbl = a; }
        if (ent != "") { Direct(ent, lbl, ver); return; }
        Prefetch();
        RefreshStatus(true);
        pages.Push(MainPage());
        if (St("setup_missing") != "") pages.Push(SetupPage());
        if (St("first_setup") != "") pages.Push(WizardPage());
        if (code == 10 || afterUpdate) ShowText(LastSession().Contains("launcher update") || afterUpdate ? "Launcher updated" : "Previous session (closed before its check)", LastSession(), null);
        Invalidate();
    }

    // Direct mode (one Steam shortcut per entry): no menu, the result closes by itself.
    void Direct(string entry, string label, string version) {
        if (entry == "version-next" || entry == "version-prev" || entry == "version-set" || entry == "notes") {
            ShowText("Skate launcher", Run(entry + (label == "" ? "" : " " + label) + VerArg(version)), null);
            CloseIn(15); return;
        }
        string name = null;
        var rows = Rows("entries" + VerArg(version), 7);
        foreach (var e in rows) if (e[0] == entry) name = e[1];
        if (name == null) { ShowText("Unknown entry", "Unknown entry: " + entry + " (" + rows.Count + " entries listed)\nCheck the shortcut's launch options (see README.md).", null); CloseIn(15); return; }
        if (entry.StartsWith("rust-")) log.Add(Run("notes" + VerArg(version)));
        StartJob(entry + (label == "" ? "" : " " + label) + VerArg(version), name);
    }
    void CloseIn(int seconds) { closeAt = seconds; closeTime = DateTime.Now.AddSeconds(seconds); }

    // ---------- pages ----------
    Page MainPage() {
        var p = new Page { Title = "Skate launcher" };
        p.OnBack = delegate { };
        var entries = Rows("entries", 7);
        p.Items.Add(new Item("Play: " + St("version"), "Our engine, the selected version (" + St("version_status") + ").", true, delegate { pages.Push(EntryPage("Play: " + St("version"), "rust")); }));
        p.Items.Add(new Item("Versions", "Pick the build Play starts.", true, delegate { pages.Push(VersionPage()); }));
        p.Items.Add(new Item("What to test", "What changed and what to try.", true, delegate { ShowText("What to test", Listed("notes"), null); }));
        bool recomp = entries.Exists(e => e[4] == "recomp"), retail = entries.Exists(e => e[4] == "retail");
        p.Items.Add(new Item("Recomp", recomp ? "Recompiled retail, traced or not." : "Not set up (see README.md).", recomp, delegate { pages.Push(EntryPage("Recomp", "recomp")); }));
        p.Items.Add(new Item("RPCS3", retail ? "The PS3 game in RPCS3." : "Not set up (see README.md).", retail, delegate { pages.Push(EntryPage("Retail Skate 3 (RPCS3)", "retail")); }));
        p.Items.Add(new Item("Tools", "Game files, builds, logs, recomp, setup.", true, delegate { pages.Push(EntryPage("Tools", "tools")); }));
        p.Items.Add(new Item("Last session", "What was checked and sent.", true, delegate { ShowText("Last session", LastSession(), null); }));
        p.Items.Add(new Item("Exit", "Close the launcher.", true, delegate { Close(); }));
        return p;
    }
    Page EntryPage(string title, string group) {
        // The engine's play screen (Play, Play + trace all, ...): buttons two wide like the main screen (laptop notes
        // 2026-10-21 22:23).
        var p = new Page { Title = title, Cols = group == "rust" ? 2 : 0 };
        foreach (var e in Rows("entries", 7)) {
            if (e[4] != group) continue;
            var row = e;
            bool on = row[5] == "1";
            var item = new Item(row[1], on ? row[2] : row[6], on, delegate {
                if (row[3] == "1") pages.Push(LabelPage(row)); else Begin(row[0], "", row[1]);
            });
            // The detail panel's text (launcher.ps1 entries columns 8 and 9; an older launcher.ps1 has neither).
            if (row.Length > 7) item.Info = row[7];
            if (row.Length > 8 && row[8] != "") item.Records = row[8].Split('|');
            p.Items.Add(item);
        }
        if (group == "rust" || group == "tools") p.Items.Add(ShareItem());
        if (group == "tools") p.Items.Add(new Item("Opt-ins and settings", "The game's opt-ins and settings, the same ones as its Guide / F1 overlay.", true, delegate { pages.Push(GameSettingsPage("Opt-ins")); }));
        if (group == "tools") p.Items.Add(SameMonitorItem());
        if (group == "tools") p.Items.Add(new Item("Set up the launcher", "Game folder, space for builds, the Skate 3 disc, the recomp and RPCS3.", true, delegate { wiz = null; pages.Push(WizardPage()); }));
        if (group == "tools") p.Items.Add(new Item("Clear builds and build caches", "Frees disk space: everything, or one version's build cache or whole build. Asks before deleting.", true, delegate { pages.Push(ClearPage()); }));
        if (p.Items.Count == 0) p.Items.Add(new Item("Nothing here yet", "See README.md to set these up.", false, null));
        return p;
    }
    // Tools > Opt-ins and settings (USER-ASKS 337): launcher.ps1 "game-settings" lists the game's saved opt-ins and settings
    // (settings\*.json, the files the game's overlay writes); pressing a row steps it ("game-setting <key>"). Opt-ins
    // first, the settings on their own page.
    Page GameSettingsPage(string group) {
        var p = new Page { Title = group == "Opt-ins" ? "Opt-ins" : "Settings" };
        foreach (var r in Rows("game-settings", 5)) {
            if (r[1] != group) continue;
            var row = r; var item = new Item(row[2] + ": " + row[3], row[4], true, null);
            item.Run = delegate {
                int code; string o = Run("game-setting " + row[0], out code);
                if (code != 0) { ShowText(row[2], o, null); return; }
                foreach (var n in Rows("game-settings", 5)) if (n[0] == row[0]) { item.Text = n[2] + ": " + n[3]; }
            };
            p.Items.Add(item);
        }
        if (group == "Opt-ins") p.Items.Add(new Item("Settings", "Resolution, render scale, framerate cap, FPS counter, volumes and difficulty.", true, delegate { pages.Push(GameSettingsPage("Settings")); }));
        if (p.Items.Count == 0) p.Items.Add(new Item("No game files yet", "Convert the game files first (Tools > setup).", false, null));
        return p;
    }
    // docs/hails-additions/second-pc-testing.md#share-sessions-via-github
    Item ShareItem() {
        var item = new Item("", "", false, null);
        item.Run = delegate {
            if (St("share").StartsWith("on ")) { Run("share-off"); RefreshStatus(); ShowShare(item); }
            else pages.Push(ConsentPage(item));
        };
        ShowShare(item);
        return item;
    }
    void ShowShare(Item item) {
        string s = St("share");
        bool on = s.StartsWith("on ");
        item.Text = s == "" ? "Share sessions via GitHub" : "Share sessions: " + (on ? "ON" : "OFF");
        item.Hint = s == "" ? "Not in this launcher version." : on ? "After each session the logs are packed and a GitHub issue opens on " + s.Substring(3) + ". Press to turn off."
            : s == "no repo" ? "This version has no GitHub repository to send to." : "Off. Press to read what is shared and turn it on.";
        item.On = s != "" && s != "no repo";
    }
    Page ConsentPage(Item toggle) {
        var p = new Page { Title = "Share sessions via GitHub" };
        p.Items.Add(new Item("Turn it on", "After each session the launcher zips the new logs and opens a GitHub issue on the repository of the version you play, in your browser. You drag the zip in and press Submit; nothing is sent without that.", true,
            delegate { Run("share-on"); RefreshStatus(); ShowShare(toggle); pages.Pop(); }));
        p.Items.Add(new Item("Not now", "Sharing stays off.", true, delegate { pages.Pop(); }));
        p.Items.Add(new Item("What is shared", "Game logs, trace folders and launcher build logs of this PC, with your Windows user name, home folder and PC name replaced. Not shared: saves, game files, recomp sound captures. Issues on public repositories are public. Turn it off here or in Tools at any time.", false, null));
        return p;
    }
    static string Gb(long bytes) { return (bytes / 1073741824.0).ToString("0.0") + " GB"; }
    // A version's build cache = its cargo target folder (it plays on, the next build starts from scratch); its build =
    // that plus bin\ (it builds again at its next launch).
    Page ClearPage() {
        var p = new Page { Title = "Clear builds and build caches" };
        long allCache = 0, allBin = 0;
        var rows = Rows("sizes", 4);
        foreach (var r in rows) { allCache += long.Parse(r[2]); allBin += long.Parse(r[3]); }
        p.Items.Add(new Item("Everything: every build and build cache (" + Gb(allCache + allBin) + ")", "Every version builds again at its next launch.", allCache + allBin > 0,
            delegate { Confirm("Delete every build and build cache?", "clear-build all", "Clear all builds"); }));
        p.Items.Add(new Item("Every build cache (" + Gb(allCache) + ")", "The versions still play; their next builds start from scratch.", allCache > 0,
            delegate { Confirm("Delete every build cache?", "clear-cache all", "Clear all build caches"); }));
        foreach (var r in rows) {
            var row = r; long cache = long.Parse(row[2]), bin = long.Parse(row[3]);
            if (cache + bin == 0) continue;   // never built here: nothing to clear
            p.Items.Add(new Item(row[1] + ": build cache (" + Gb(cache) + ")", cache > 0 ? "It still plays; its next build starts from scratch." : "Nothing to clear.", cache > 0,
                delegate { Confirm("Delete the build cache of " + row[1] + "?", "clear-cache " + row[0], "Clear build cache: " + row[1]); }));
            p.Items.Add(new Item(row[1] + ": whole build (" + Gb(cache + bin) + ")", cache + bin > 0 ? "Build cache and bin\\: it builds again at its next launch." : "Nothing to clear.", cache + bin > 0,
                delegate { Confirm("Delete the whole build of " + row[1] + "?", "clear-build " + row[0], "Clear build: " + row[1]); }));
        }
        return p;
    }
    void Confirm(string question, string command, string title) {
        var p = new Page { Title = question };
        p.Items.Add(new Item("No, keep it", "", true, delegate { pages.Pop(); }));
        p.Items.Add(new Item("Yes, delete", "This can't be undone; the files are build output only.", true, delegate { pages.Pop(); pages.Pop(); log.Clear(); StartJob(command, title); }));
        pages.Push(p);
    }
    Page LabelPage(string[] e) {
        var p = new Page { Title = "Label for the session folder" };
        foreach (var l in Labels) { string lbl = l; p.Items.Add(new Item(l, l == "session" ? "No label." : "", true, delegate { pages.Pop(); Begin(e[0], lbl == "session" ? "" : lbl, e[1]); })); }
        return p;
    }
    Page VersionPage() {
        var p = new Page { Title = "Choose version" };
        foreach (var v in Rows("versions", 3)) {
            var row = v;
            p.Items.Add(new Item((row.Length > 3 && row[3] == "*" ? "> " : "") + row[1], row[2], true, delegate {
                pages.Pop(); switching = true; Invalidate();
                Task.Run(() => Run("version-set " + row[0])).ContinueWith(t => BeginInvoke((Action)RebuildMain)); }));
            if (row.Length > 3 && row[3] == "*") p.Sel = p.Items.Count - 1;
        }
        return p;
    }
    Page SetupPage() {
        var p = new Page { Title = "First-time setup" };
        var sb = new StringBuilder("This PC needs a few tools before it can build and play versions:\n\n");
        foreach (var r in Rows("setup-check", 5))
            sb.Append((r[2] == "1" ? "  ok       " : "  MISSING  ") + r[1] + (r[2] == "1" ? "" : (r[4] == "1" ? "" : " (optional)")) + "\n");
        p.Body = sb.ToString();
        p.Items.Add(new Item("Install what is missing", "Windows asks once for permission (UAC). The C++ build tools take 10 minutes or more.", true, delegate { pages.Pop(); Begin("tool-setup", "", "First-time setup"); }));
        p.Items.Add(new Item("Not now", "The Play entries stay greyed out until the tools are there.", true, delegate { pages.Pop(); }));
        return p;
    }

    // First-time setup (USER-ASKS 461): where the game versions and builds go, the space they get, the Skate 3 disc, the
    // recomp and RPCS3. launcher.ps1 "config-get" reads the values, "config-set" writes the changed ones to config.json.
    Dictionary<string, string> wiz, wizStart;
    static readonly int[] CapChoices = { 0, 50, 100, 150, 200, 300, 500 };
    string W(string k) { string v; return wiz != null && wiz.TryGetValue(k, out v) ? v : ""; }
    static string Or(string a, string b) { return a == "" ? b : a; }
    static string Now(string v, string unset) { return "Now: " + Or(v, unset) + "\n\n"; }
    static string FreeOn(string path) {
        try {
            string root = Path.GetPathRoot(Path.GetFullPath(path));
            return "Free on " + root + ": " + (new DriveInfo(root).AvailableFreeSpace >> 30) + " GB.";
        } catch { return ""; }
    }
    Page WizardPage() {
        if (wiz == null) {
            wiz = new Dictionary<string, string>();
            foreach (var r in Rows("config-get", 2)) wiz[r[0]] = r[1];
            if (W("storage_root") == "") wiz["storage_root"] = Or(W("target_root"), W("default_root"));
            wizStart = new Dictionary<string, string>(wiz);
        }
        var p = new Page { Title = "Set up the launcher" };
        string root = W("storage_root");
        int cap; int.TryParse(W("build_cap_gb"), out cap);
        p.Items.Add(new Item("Game folder", Now(root, "not set") + (W("standalone") == "1" ? "Where the launcher keeps the game versions and their builds" : "Where the launcher keeps the builds") + " (one version with its build takes about 15 to 45 GB). " + FreeOn(root), true, delegate { PickFolder("storage_root", "Choose the folder for the game versions and their builds"); }));
        p.Items.Add(new Item("Space for builds", Now(cap > 0 ? cap + " GB" : "no limit", "") + "Press to change. Before a build would go over it, the launcher deletes the build cache of the version built longest ago. " + FreeOn(root), true, delegate {
            int i = Array.IndexOf(CapChoices, cap);
            wiz["build_cap_gb"] = CapChoices[(i + 1) % CapChoices.Length].ToString(); ReshowWizard();
        }));
        p.Items.Add(new Item("Skate 3 disc", Now(W("disc"), "not set") + "Your Xbox 360 Skate 3 ISO or default.xex. The first play converts the game files from it once.", true, delegate { PickFile("disc", "Choose your Skate 3 ISO or default.xex", "Skate 3 (*.iso, default.xex)|*.iso;default.xex|All files (*.*)|*.*"); }));
        p.Items.Add(new Item("Skate 3 Recompiled", Now(W("recomp.working_dir"), "not set (optional)") + "The skate3recomp game folder (with skate3.exe). Adds the Recomp entries.", true, delegate { PickRecomp(); }));
        p.Items.Add(new Item("RPCS3", Now(W("rpcs3.exe"), "not set (optional)") + "rpcs3.exe, to play retail Skate 3 (PS3) for comparison.", true, delegate { PickFile("rpcs3.exe", "Choose rpcs3.exe", "RPCS3 (rpcs3.exe)|rpcs3.exe|Programs (*.exe)|*.exe"); }));
        p.Items.Add(new Item("RPCS3 Skate 3 game", Now(W("rpcs3.game"), "not set (optional)") + "The folder of your Skate 3 PS3 game (the one with PS3_GAME or EBOOT.BIN).", true, delegate { PickFolder("rpcs3.game", "Choose your Skate 3 PS3 game folder"); }));
        p.Items.Add(new Item("Done", "Saves these settings. Change them later in Tools > Set up the launcher.", true, delegate { SaveWizard(); }));
        p.Items.Add(new Item("Not now", "Saves nothing; this page opens again at the next start.", true, delegate { wiz = null; pages.Pop(); Invalidate(); }));
        p.OnBack = delegate { wiz = null; pages.Pop(); Invalidate(); };
        return p;
    }
    void ReshowWizard() {
        int sel = pages.Peek().Sel; pages.Pop();
        var p = WizardPage(); p.Sel = Math.Min(sel, p.Items.Count - 1); pages.Push(p); Invalidate();
    }
    void PickFolder(string key, string title) {
        using (var d = new FolderBrowserDialog { Description = title, ShowNewFolderButton = true }) {
            if (Directory.Exists(W(key))) d.SelectedPath = W(key);
            if (d.ShowDialog(this) == DialogResult.OK) { wiz[key] = d.SelectedPath; ReshowWizard(); }
        }
    }
    string AskFile(string current, string title, string filter) {
        using (var d = new OpenFileDialog { Title = title, Filter = filter, CheckFileExists = true }) {
            try { if (current != "" && Directory.Exists(Path.GetDirectoryName(current))) d.InitialDirectory = Path.GetDirectoryName(current); } catch { }
            return d.ShowDialog(this) == DialogResult.OK ? d.FileName : null;
        }
    }
    void PickFile(string key, string title, string filter) {
        string f = AskFile(W(key), title, filter);
        if (f != null) { wiz[key] = f; ReshowWizard(); }
    }
    // The recomp's game folder; skate3.exe is taken from it (or a folder under it), else asked for.
    void PickRecomp() {
        string dir;
        using (var d = new FolderBrowserDialog { Description = "Choose your Skate 3 Recompiled game folder" }) {
            if (Directory.Exists(W("recomp.working_dir"))) d.SelectedPath = W("recomp.working_dir");
            if (d.ShowDialog(this) != DialogResult.OK) return;
            dir = d.SelectedPath;
        }
        string exe = Path.Combine(dir, "skate3.exe");
        if (!File.Exists(exe)) {
            exe = null;
            try { var found = Directory.GetFiles(dir, "skate3.exe", SearchOption.AllDirectories); if (found.Length > 0) exe = found[0]; } catch { }
            if (exe == null) exe = AskFile(dir + "\\x", "Choose the recomp's skate3.exe", "skate3.exe|skate3.exe|Programs (*.exe)|*.exe");
            if (exe == null) return;
        }
        wiz["recomp.working_dir"] = dir; wiz["recomp.exe"] = exe; ReshowWizard();
    }
    static string Js(string v) { return "\"" + v.Replace("\\", "\\\\").Replace("\"", "\\\"") + "\""; }
    void SaveWizard() {
        var parts = new List<string>();
        foreach (var k in new[] { "disc", "recomp.exe", "recomp.working_dir", "rpcs3.exe", "rpcs3.game" })
            if (W(k) != wizStart[k]) parts.Add(Js(k) + ": " + Js(W(k)));
        if (W("build_cap_gb") != wizStart["build_cap_gb"]) parts.Add(Js("build_cap_gb") + ": " + W("build_cap_gb"));
        string root = W("storage_root");
        if (root != wizStart["storage_root"] && root != "") {
            parts.Add(Js("storage_root") + ": " + Js(root));
            parts.Add(Js("target_root") + ": " + Js(Path.Combine(root, "builds")));
            parts.Add(Js("build_cap_dir") + ": " + Js(root));
            if (W("standalone") == "1") parts.Add(Js("versions_root") + ": " + Js(Path.Combine(root, "versions")));
        }
        parts.Add(Js("first_setup_done") + ": true");
        string file = Path.Combine(StateDir(), "config-set.json");
        File.WriteAllText(file, "{" + string.Join(", ", parts.ToArray()) + "}", new UTF8Encoding(false));
        string r = Run("config-set \"" + file + "\"");
        wiz = null; pages.Pop();
        if (r.Trim() != "saved") ShowText("Set up the launcher", "The settings could not be saved:\n\n" + r, null);
        RebuildMain();
        Invalidate();
    }
    void RebuildMain() {
        // A version change or a finished job can change every list (entries, versions, notes): fetch them again.
        // The menu is rebuilt when they are in (off the window thread; the newest request wins).
        ForgetCache();
        Prefetch();
        int gen = ++rebuildGen;
        WhenListed(delegate {
            if (gen != rebuildGen) return;
            switching = false;
            RefreshStatus(true);
            var all = new List<Page>(pages); all.Reverse();
            pages.Clear();
            foreach (var pg in all) {
                if (pg.Title == "Skate launcher" && pg.OnBack != null) { var m = MainPage(); m.Sel = Math.Min(pg.Sel, m.Items.Count - 1); pages.Push(m); }
                else pages.Push(pg);
            }
            Invalidate();
        });
    }
    int rebuildGen;
    // A version switch until the menu is rebuilt for it: Play waits for it (no start of the old version).
    bool switching;
    void ShowText(string title, string body, Action back) { ShowText(title, body, back, null); }
    void ShowText(string title, string body, Action back, string mono) {
        var p = new Page { Title = title, Body = body, Mono = mono };
        p.OnBack = back;
        pages.Push(p);
        Invalidate();
    }

    // ---------- jobs ----------
    void Begin(string entry, string label, string name) {
        if (switching) return;
        string g = RunningGames();
        if (g != "" && !entry.StartsWith("tool-")) {
            waitEntry = entry; waitLabel = label; waitName = name;
            ShowText("Waiting", "Already running: " + g + "\nOnly one game at a time. Close it, or wait here: this continues by itself.\n\nB: cancel", delegate { waitEntry = null; pages.Pop(); });
            return;
        }
        log.Clear();
        StartJob(entry + (label == "" ? "" : " " + label), name);
    }
    void StartJob(string args, string title) {
        jobTitle = title; jobHidGame = false; cancelArmed = cancelled = cancelledInFetch = false; gameWindow = IntPtr.Zero; jobStart = DateTime.Now;
        build.Close(); build.LogPath = null; build.Ended = false; build.Active = false; build.PreWait = null; build.LastPreWait = null;
        // Let the game take the foreground from us (Steam gives the foreground window its controller layout).
        Native.AllowSetForegroundWindow(-1);
        job = new Process { StartInfo = Ps(args + " -NoPrompt"), EnableRaisingEvents = true };
        // Build output feeds the build view; other lines the job log. Drawn from the timer, not per line.
        DataReceivedEventHandler add = delegate(object s, DataReceivedEventArgs e) { if (e.Data != null) BeginInvoke((Action)delegate {
            if (!build.Feed(e.Data)) { string c = BuildView.Clean(e.Data); if (c != "") { log.Add(c); if (log.Count > 400) log.RemoveAt(0); } }
            dirty = true; }); };
        job.OutputDataReceived += add; job.ErrorDataReceived += add;
        job.Exited += delegate { BeginInvoke((Action)JobDone); };
        job.Start(); job.StandardInput.Close();
        job.BeginOutputReadLine(); job.BeginErrorReadLine();
        Invalidate();
    }
    void JobDone() {
        job = null;
        if (!Visible) Show();
        if (WindowState == FormWindowState.Minimized) WindowState = FormWindowState.Normal;
        Activate(); Native.Front(Handle);
        string text = LastSession();
        build.Close();
        if (direct.Length == 0 && RestartIfUpdated(true)) return;
        string mono = null;
        if (cancelled && cancelledInFetch) text = "Cancelled after " + BuildView.Clock(DateTime.Now - build.PreWaitSince) + " while " + build.PreWait + ". Nothing was built or changed.";
        else if (cancelled) text = "Build cancelled after " + BuildView.Clock(DateTime.Now - build.Start) + " (it was waiting for another build)." + (build.LogPath != null ? "\nBuild log: " + build.LogPath : "");
        else if (build.LogPath != null && build.Ended) {
            string took = BuildView.Clock(build.End - build.Start);
            if (build.Ok) text += "\n\nBuilt " + build.Name + " at " + build.Commit + " in " + took + (build.Warnings > 0 ? ", " + build.Warnings + " warnings" : "") + ".\nBuild log: " + build.LogPath;
            else { text += "\n\nBuild failed after " + took + ". Build log: " + build.LogPath + "\n\nFirst error:"; mono = string.Join("\n", build.FirstError()); }
        }
        bool bad = text.Contains("MALFORMED") || text.Contains("No new") || text.Contains("Not started") || text.Contains("panic(s)") || text.Contains("failed") || text.Contains("Still missing");
        bad = bad || mono != null;
        if (direct.Length > 0) { ShowText(bad ? "Finished, check this" : "Finished", text, null, mono); CloseIn(10); return; }
        RebuildMain();
        ShowText(bad ? "Finished, check this" : "Finished", text, null, mono);
    }
    // While a job runs: hide once the game's window shows, so it gets the foreground and the controller layout. The
    // first window is the startup splash; the game's own window comes when the splash is done, and gets the front too.
    void WatchGame() {
        if (job == null || (jobHidGame && (DateTime.Now - hidAt).TotalSeconds > 300)) return;
        foreach (var n in GameNames) foreach (var p in Process.GetProcessesByName(n)) {
            IntPtr w;
            using (p) w = p.MainWindowHandle;
            if (w == IntPtr.Zero || w == gameWindow || !Native.IsWindowVisible(w)) continue;
            gameWindow = w; Native.Front(w);
            if (!jobHidGame) { jobHidGame = true; hidAt = DateTime.Now; Hide(); }
            return;
        }
    }

    // ---------- input ----------
    static ushort Pad() {
        for (int i = 0; i < 4; i++) {
            Native.XState s;
            try {
                if (Native.XInputGetState(i, out s) == 0) {
                    ushort b = s.Buttons;
                    if (s.LY > 16000) b |= UP; if (s.LY < -16000) b |= DOWN;
                    if (s.LX < -16000) b |= LEFT; if (s.LX > 16000) b |= RIGHT;
                    return b;
                }
            } catch { return 0; }
        }
        return 0;
    }
    DateTime scannedAt;
    void Poll() {
        if (powerOffAt >= 0 && !poweredOff) {
            if ((float)clock.Elapsed.TotalSeconds - powerOffAt >= OffTime) { poweredOff = true; Close(); return; }
            Invalidate(); return;
        }
        if (closeAt >= 0) {
            int left = (int)Math.Ceiling((closeTime - DateTime.Now).TotalSeconds);
            if (left <= 0) { Close(); return; }
            if (left != closeAt) { closeAt = left; Invalidate(); }
        }
        // Process scans 4 times a second, not every 16 ms frame: each one lists every process on the PC (USER-ASKS 436).
        if ((DateTime.Now - scannedAt).TotalMilliseconds >= 250) {
            scannedAt = DateTime.Now;
            if (waitEntry != null && RunningGames() == "") { string e = waitEntry; waitEntry = null; pages.Pop(); Begin(e, waitLabel, waitName); }
            WatchGame();
        }
        if (dirty || Animating()) { dirty = false; Invalidate(); }
        // The rolling boards and the Play glow: about 30 frames a second while the menu or a job shows.
        if (Visible && ++frame % 2 == 0) Invalidate();
        // Only while the menu is in front: XInput reports the pad to every process, so the game's presses would
        // otherwise drive the menu behind it.
        if (job != null && Form.ActiveForm == this && ((build.Waiting != null && !build.Ended) || build.FetchWaiting())) {
            ushort jb = Pad(); if ((jb & ~prevPad & B) != 0) CancelPress(); prevPad = jb; return;
        }
        if (starting && Form.ActiveForm == this) { ushort sb = Pad(); if ((sb & ~prevPad & B) != 0) SkipPress(); prevPad = sb; return; }
        if (job != null || Form.ActiveForm != this) { prevPad = Pad(); return; }
        ushort b = Pad(), pressed = (ushort)(b & ~prevPad);
        if ((pressed & A) != 0) Act("ok", true);
        else if ((pressed & B) != 0) Act("back", true);
        else if ((pressed & UP) != 0) Act("up", true);
        else if ((pressed & DOWN) != 0) Act("down", true);
        else if ((pressed & LEFT) != 0) Act("left", true);
        else if ((pressed & RIGHT) != 0) Act("right", true);
        else if ((b & (UP | DOWN)) != 0 && b == prevPad) { if (++held > 14 && held % 4 == 0) Act((b & UP) != 0 ? "up" : "down", true); }
        if (b != prevPad) held = 0;
        prevPad = b;
    }
    // Cancelling a build that waits for another one: the first B arms, the second kills the job's process tree
    // (powershell launcher.ps1 -> Build.ps1 -> cargo); a cargo that only waits on a lock loses nothing.
    void CancelPress() {
        if (job == null) return;
        if (!cancelArmed) { cancelArmed = true; dirty = true; return; }
        cancelled = true; cancelledInFetch = build.PreWait != null; build.Note("cancelled after " + BuildView.Clock(DateTime.Now - build.Start));
        try { Process.Start(new ProcessStartInfo("taskkill.exe", "/T /F /PID " + job.Id) { CreateNoWindow = true, UseShellExecute = false }); } catch { }
    }
    protected override bool ProcessCmdKey(ref Message msg, Keys key) {
        if (job != null && ((build.Waiting != null && !build.Ended) || build.FetchWaiting()) && (key == Keys.Escape || key == Keys.Back)) { CancelPress(); return true; }
        if (starting && (key == Keys.Escape || key == Keys.Back)) { SkipPress(); return true; }
        if (job == null) {
            switch (key) {
                case Keys.Up: case Keys.W: Act("up", false); return true;
                case Keys.Down: case Keys.S: Act("down", false); return true;
                case Keys.Left: case Keys.A: Act("left", false); return true;
                case Keys.Right: case Keys.D: Act("right", false); return true;
                case Keys.Enter: Act("ok", false); return true;
                case Keys.Escape: case Keys.Back: case Keys.Space: Act("back", false); return true;
            }
        }
        return base.ProcessCmdKey(ref msg, key);
    }
    // Mouse: hovering selects a row or tile, a left click runs it, a right click goes back. The rectangles come
    // from the last paint (hits), so they always match what is on screen.
    readonly List<KeyValuePair<RectangleF, int>> hits = new List<KeyValuePair<RectangleF, int>>();
    int HitAt(Point pt) {
        foreach (var h in hits) if (h.Key.Contains(pt)) return h.Value;
        return -1;
    }
    // The A / B prompts drawn last paint (Prompt), clickable like the buttons they show.
    readonly List<KeyValuePair<RectangleF, string>> promptHits = new List<KeyValuePair<RectangleF, string>>();
    string hoverPrompt;
    string PromptAt(Point pt) {
        foreach (var h in promptHits) if (h.Key.Contains(pt)) return h.Value;
        return null;
    }
    // A clicked prompt does what its button does in the current state (ProcessCmdKey's Back / Enter).
    void PromptPress(string key) {
        if (key == "B") {
            if (job != null && ((build.Waiting != null && !build.Ended) || build.FetchWaiting())) CancelPress();
            else if (starting) SkipPress();
            else if (job == null) Act("back", false);
        }
        else if (key == "A" && job == null) Act("ok", false);
        // The camcorder's keys: MENU = back, the dial and the zoom rocker move, START/STOP plays, POWER closes.
        else if (key == "MENU") PromptPress("B");
        else if (job == null && (key == "U" || key == "D" || key == "L" || key == "R")) Act(key == "U" ? "up" : key == "D" ? "down" : key == "L" ? "left" : "right", false);
        else if (key == "PLAY" && job == null && !starting && pages.Count > 0) { var p = pages.Peek(); if (IsMain(p) && p.Items.Count > 0) p.Sel = 0; Act("ok", false); }
        else if (key == "POWER") Close();
    }
    protected override void OnMouseMove(MouseEventArgs e) {
        base.OnMouseMove(e);
        string over = PromptAt(e.Location);
        if (over != hoverPrompt) { hoverPrompt = over; Cursor = over != null ? Cursors.Hand : Cursors.Default; Invalidate(); }
        int i = HitAt(e.Location);
        if (job == null && i >= 0 && pages.Count > 0 && pages.Peek().Sel != i) { pages.Peek().Sel = i; Invalidate(); }
    }
    protected override void OnMouseClick(MouseEventArgs e) {
        base.OnMouseClick(e);
        string prompt = e.Button == MouseButtons.Left ? PromptAt(e.Location) : null;
        if (prompt != null) { PromptPress(prompt); return; }
        if (job != null) return;
        if (e.Button == MouseButtons.Right) { Act("back", false); return; }
        if (e.Button != MouseButtons.Left) return;
        int i = HitAt(e.Location);
        if (i >= 0 && pages.Count > 0) { pages.Peek().Sel = i; Act("ok", false); }
        else if (closeAt >= 0) Close();
    }
    // One action; the same action from the OTHER source (pad / keys) within 150 ms is dropped: Steam can send a key
    // for a press that XInput also reports.
    void Act(string a, bool fromPad) {
        if (a == lastAction && fromPad != lastFromPad && (DateTime.Now - lastActionTime).TotalMilliseconds < 150) return;
        lastAction = a; lastFromPad = fromPad; lastActionTime = DateTime.Now;
        if (closeAt >= 0) { Close(); return; }
        if (pages.Count == 0) return;
        var p = pages.Peek();
        if (a == "back") {
            if (p.OnBack != null) p.OnBack();
            else pages.Pop();
            if (pages.Count == 0) pages.Push(MainPage());
        } else if (p.Items.Count > 0) {
            // "ok" first: the main page's grid move used to swallow it, so A / Enter did nothing there (laptop 2026-10-18).
            if (a == "ok") { var it = p.Items[p.Sel]; if (it.On && it.Run != null) it.Run(); }
            // Every screen is one of the camcorder's menu lists (W / T on the zoom rocker step like the jog dial).
            else if (a == "up" || a == "left") p.Sel = (p.Sel - 1 + p.Items.Count) % p.Items.Count;
            else if (a == "down" || a == "right") p.Sel = (p.Sel + 1) % p.Items.Count;
        } else if (a == "ok" && p.Body != null) {
            if (p.OnBack != null) p.OnBack(); else pages.Pop();
        }
        Invalidate();
    }

    // ---------- drawing helpers ----------
    readonly Stopwatch clock = Stopwatch.StartNew();
    DateTime jobStart;
    static float Ease(float x) { x = Math.Max(0, Math.Min(1, x)); float r = 1 - x; return 1 - r * r * r; }
    static bool IsMain(Page p) { return p.Title == "Skate launcher" && p.OnBack != null; }
    // Every list page (no body text, not the main screen) is a grid of buttons two wide with the detail panel beside it
    // (user 2026-10-21: the play screen's layout for the rest of the launcher).
    static int GridCols(Page p) { return p.Cols > 0 ? p.Cols : (p.Body == null && !IsMain(p) ? 2 : 0); }

    static System.Drawing.Drawing2D.GraphicsPath Round(RectangleF r, float rad) {
        var path = new System.Drawing.Drawing2D.GraphicsPath();
        float d = Math.Min(rad * 2, Math.Min(r.Width, r.Height));
        path.AddArc(r.X, r.Y, d, d, 180, 90); path.AddArc(r.Right - d, r.Y, d, d, 270, 90);
        path.AddArc(r.Right - d, r.Bottom - d, d, d, 0, 90); path.AddArc(r.X, r.Bottom - d, d, d, 90, 90);
        path.CloseFigure();
        return path;
    }
    static void Card(Graphics g, RectangleF r, float rad, Color top, Color bottom, Color edge, float edgeW) {
        using (var path = Round(r, rad)) {
            using (var b = new System.Drawing.Drawing2D.LinearGradientBrush(r, top, bottom, 90f)) g.FillPath(b, path);
            if (edgeW > 0) using (var pen = new Pen(edge, edgeW)) g.DrawPath(pen, path);
        }
    }
    static Color Fade(Color c, int alpha) { return Color.FromArgb(alpha, c.R, c.G, c.B); }
    static void Write(Graphics g, string s, Font f, Color c, RectangleF r, StringAlignment h, StringAlignment v, bool wrap) {
        using (var b = new SolidBrush(c)) using (var sf = new StringFormat()) {
            sf.Alignment = h; sf.LineAlignment = v; sf.Trimming = StringTrimming.EllipsisCharacter;
            // Wrapped text shows whole lines only (a half-clipped last line reads as noise).
            sf.FormatFlags = wrap ? StringFormatFlags.LineLimit : StringFormatFlags.NoWrap;
            g.DrawString(s ?? "", f, b, r, sf);
        }
    }

    // The splash's board (splash.rs draw_board), side view, 180 units long, its top-left at (x, y), `len` px long.
    // The board's x while it rolls along a track on a loop (in at the left, out at the right, again), independent of
    // the progress (user 2026-10-18: it should not follow the loading bar).
    static float Rolling(RectangleF track, float boardLen, float t) {
        float span = track.Width + boardLen;
        return track.X - boardLen + (t * track.Width * 0.3f) % span;
    }
    static void Board(Graphics g, float x, float y, float len) {
        var st = g.Save();
        g.TranslateTransform(x, y); g.ScaleTransform(len / 180f, len / 180f);
        const float l = 180f;
        Func<float, PointF[]> deck = delegate(float down) {
            float[,] top = { { 1, 3 }, { 5, 3.6f }, { 11, 5.6f }, { 18, 8.6f }, { 25, 11 }, { 31, 12 }, { l - 33, 12 }, { l - 26, 10.8f }, { l - 18, 7.8f }, { l - 10, 4.4f }, { l - 4, 2.4f }, { l - 1, 2 } };
            var pts = new PointF[12]; for (int i = 0; i < 12; i++) pts[i] = new PointF(top[i, 0], top[i, 1] + down); return pts;
        };
        Func<float, float, PointF[]> outline = delegate(float upper, float lower) {
            var a = deck(upper); var b = deck(lower); Array.Reverse(b);
            var all = new PointF[a.Length + b.Length]; a.CopyTo(all, 0); b.CopyTo(all, a.Length); return all;
        };
        Action<Color, PointF[]> poly = delegate(Color c, PointF[] pts) { using (var br = new SolidBrush(c)) g.FillPolygon(br, pts); };
        Action<Color, float, float, float> disc = delegate(Color c, float cx, float cy, float r) { using (var br = new SolidBrush(c)) g.FillEllipse(br, cx - r, cy - r, 2 * r, 2 * r); };
        using (var sh = new SolidBrush(Color.FromArgb(110, 0, 0, 0))) g.FillEllipse(sh, 10, 32.5f, l - 20, 5);
        var wood = outline(0, 5);
        poly(Color.FromArgb(214, 176, 124), wood);
        using (var pen = new Pen(Color.FromArgb(92, 62, 36), 0.5f)) g.DrawPolygon(pen, wood);
        poly(Color.FromArgb(150, 104, 62), outline(2.6f, 3.4f));
        poly(Color.FromArgb(34, 34, 38), outline(0, 1.4f));
        foreach (float ax in new float[] { 40, l - 42 }) {
            poly(Color.FromArgb(176, 181, 191), new[] { new PointF(ax - 10, 17), new PointF(ax + 10, 17), new PointF(ax + 9, 19.5f), new PointF(ax - 9, 19.5f) });
            poly(Color.FromArgb(176, 181, 191), new[] { new PointF(ax - 6, 19.5f), new PointF(ax + 6, 19.5f), new PointF(ax + 3.5f, 26), new PointF(ax - 3.5f, 26) });
            disc(Color.FromArgb(168, 162, 148), ax, 26.5f, 7.5f); disc(Color.FromArgb(240, 235, 220), ax, 26.5f, 6.9f);
            disc(Color.FromArgb(86, 146, 214), ax, 26.5f, 3.9f); disc(Color.FromArgb(52, 54, 60), ax, 26.5f, 1.7f);
        }
        g.Restore(st);
    }

    // Log lines as a TV can read them: no colour codes, only the text after the last carriage return.
    static readonly System.Text.RegularExpressions.Regex Ansi = new System.Text.RegularExpressions.Regex("\x1B\\[[0-9;?]*[A-Za-z]");
    static string Clean(string s) { s = Ansi.Replace(s ?? "", ""); int r = s.LastIndexOf('\r'); return (r >= 0 ? s.Substring(r + 1) : s).Trim(); }

    // ---------- the camcorder's LCD panel (USER-ASKS 484, 486, 487, 490) ----------
    // The window is only the flip-out LCD panel of a mid-2000s HD camcorder, seen first-person: the screen, its full
    // bezel carrying the panel's own keys, and the hinge on the right edge where the camera body is. It swings open from
    // edge-on at the start and shut at the close. Drawn in a 1240 x 760 unit space into a per-pixel-alpha (layered)
    // window. Layout, keys, timings and the sticker: docs/hails-additions/41-launcher-camcorder-viewfinder.md.
    const float DevW = 1240, DevH = 760;
    static readonly RectangleF PanelR = new RectangleF(20, 20, 1172, 720), ScreenR = new RectangleF(172, 52, 990, 557);
    static readonly Color Plastic = Color.FromArgb(54, 56, 62), PlasticHi = Color.FromArgb(100, 103, 112), PlasticDark = Color.FromArgb(24, 25, 29),
        Print = Color.FromArgb(200, 202, 208), RecRed = Color.FromArgb(230, 40, 32);
    static readonly Font UTiny = new Font("Tahoma", 13, GraphicsUnit.Pixel), USmall = new Font("Tahoma", 16, FontStyle.Bold, GraphicsUnit.Pixel),
        UKey = new Font("Tahoma", 19, FontStyle.Bold, GraphicsUnit.Pixel), UModel = new Font("Tahoma", 18, FontStyle.Bold, GraphicsUnit.Pixel);
    float devS = 1, devX, devY;   // units -> window pixels, from the last Render
    RectangleF ToWin(RectangleF r) { return new RectangleF(devX + r.X * devS, devY + r.Y * devS, r.Width * devS, r.Height * devS); }
    void Key(RectangleF unit, string key) { promptHits.Add(new KeyValuePair<RectangleF, string>(ToWin(unit), key)); }
    static RectangleF Grow(RectangleF r, float d) { return new RectangleF(r.X - d, r.Y - d, r.Width + 2 * d, r.Height + 2 * d); }
    static float Clamp01(float x) { return Math.Max(0, Math.Min(1, x)); }

    // Power, on one clock (Assembly, seconds 0..OnTotal): the panel swings out from edge-on and settles facing you with
    // a little wobble, then the backlight comes on (sometimes with a flicker or two). Closing plays it backwards, a
    // little faster. Every open and close rolls its own swing time, wobble, delay and flicker (Vary); snapshots use the
    // plain values.
    const float OnTotal = 1.2f, OffSpeed = 1.3f, OffTime = OnTotal / OffSpeed + 0.05f;
    float powerOnAt = -1, powerOffAt = -1, snapPanel = -1; bool poweredOff, snapshotMode; int snapSeed = 3;
    float Assembly(float t) {
        if (snapPanel >= 0) return snapPanel * OnTotal;
        if (powerOffAt >= 0) return Math.Max(0, OnTotal - (t - powerOffAt) * OffSpeed);
        return powerOnAt < 0 ? 0 : Math.Min(OnTotal, t - powerOnAt);
    }
    static float BackOut(float x, float k) { x = Clamp01(x) - 1; return 1 + (k + 1) * x * x * x + k * x * x; }
    readonly float[] vary = { 1, 0.6f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 150, 0, 6.5f };
    // The board's speed, place and rhythm are rolled at the start only (boardToo): re-rolling them for the close made
    // the board jump while the screen faded out.
    void Vary(Random r, bool boardToo) {
        Func<double, double, float> u = delegate(double lo, double hi) { return (float)(lo + r.NextDouble() * (hi - lo)); };
        vary[0] = u(0.85, 1.15); vary[1] = u(0.2, 1.2); vary[2] = u(-0.04, 0.06); vary[3] = r.Next(3); vary[4] = u(0, 1);
        if (boardToo) { vary[17] = u(120, 190); vary[18] = u(0, 400); vary[19] = u(5.5, 8); }
    }
    float SwingTo { get { return 0.08f + 0.6f * vary[0]; } }
    // 0 = edge-on at the hinge, 1 = facing you (the wobble overshoots a little past flat, then settles).
    float PanelOpen(float t) { return BackOut(Clamp01((Assembly(t) - 0.08f) / (SwingTo - 0.08f)), vary[1]); }
    float ScreenLevel(float t) {
        float level = Clamp01((Assembly(t) - SwingTo - 0.1f - vary[2]) / 0.25f);
        if (vary[3] >= 1 && level > 0 && level < 1) { int q = (int)(level * 6 + vary[4]); if (q % 3 == 1 && q < 3 * (int)vary[3] + 1) level *= 0.25f; }
        return level;
    }
    float WindowAlpha(float t) { return snapshotMode ? 1 : Clamp01(Assembly(t) / 0.06f); }
    bool Animating() { float t = (float)clock.Elapsed.TotalSeconds; return powerOnAt < 0 || t - powerOnAt < OnTotal + 0.1f || powerOffAt >= 0; }

    // The layered window: every frame is drawn into one bitmap and handed to Windows with its alpha (Native.Push).
    protected override CreateParams CreateParams { get { var cp = base.CreateParams; cp.ExStyle |= 0x80000; return cp; } }
    Bitmap frameBmp; bool needPresent = true;
    protected override void OnInvalidated(InvalidateEventArgs e) { base.OnInvalidated(e); needPresent = true; }
    protected override void OnPaint(PaintEventArgs e) { }
    protected override void OnPaintBackground(PaintEventArgs e) { }
    void Present() {
        if (!IsHandleCreated || !Visible) return;
        needPresent = false;
        float t = (float)clock.Elapsed.TotalSeconds;
        if (powerOnAt < 0) { powerOnAt = t; Vary(new Random(), true); }
        int w = ClientSize.Width, h = ClientSize.Height;
        if (frameBmp == null || frameBmp.Width != w || frameBmp.Height != h) { if (frameBmp != null) frameBmp.Dispose(); frameBmp = new Bitmap(w, h, PixelFormat.Format32bppPArgb); }
        using (var g = Graphics.FromImage(frameBmp)) { g.Clear(Color.Transparent); Render(g, w, h); }
        Native.Push(Handle, frameBmp, Left, Top, (byte)(255 * WindowAlpha(t)));
    }
    protected override void OnFormClosing(FormClosingEventArgs e) {
        if (Visible && !poweredOff && (e.CloseReason == CloseReason.UserClosing || e.CloseReason == CloseReason.None || e.CloseReason == CloseReason.TaskManagerClosing)) {
            e.Cancel = true;
            if (powerOffAt < 0) { powerOffAt = (float)clock.Elapsed.TotalSeconds; Vary(new Random(), false); if (!tick.Enabled) tick.Start(); Invalidate(); }
            return;
        }
        base.OnFormClosing(e);
    }
    // Drag the panel by its bezel.
    protected override void OnMouseDown(MouseEventArgs e) {
        base.OnMouseDown(e);
        if (e.Button == MouseButtons.Left && PromptAt(e.Location) == null && HitAt(e.Location) < 0) { Native.ReleaseCapture(); Native.SendMessage(Handle, 0xA1, (IntPtr)2, IntPtr.Zero); }
    }

    float fixedT = -1;   // --wear-test: every frame at the same moment, so only the marks differ
    void Render(Graphics g, int w, int h) {
        g.SmoothingMode = SmoothingMode.AntiAlias;
        g.TextRenderingHint = System.Drawing.Text.TextRenderingHint.AntiAliasGridFit;
        hits.Clear(); promptHits.Clear();
        float t = fixedT >= 0 ? fixedT : (float)clock.Elapsed.TotalSeconds;
        devS = Math.Min(w / DevW, h / DevH); devX = (w - DevW * devS) / 2; devY = (h - DevH * devS) / 2;
        EnsureFlip(t);
        var st = g.Save();
        g.TranslateTransform(devX, devY); g.ScaleTransform(devS, devS);
        float open = PanelOpen(t);
        if (open > 0.01f) Hinge(g);
        if (Math.Abs(open - 1) < 0.002f) LcdPanel(g, ScreenLevel(t), t, true); else FlipPanel(g, open, t);
        g.Restore(st);
    }

    static void Cylinder(Graphics g, RectangleF r, float rad, bool upright) {
        using (var path = Round(r, rad)) using (var b = new LinearGradientBrush(Grow(r, 1), PlasticDark, PlasticDark, upright ? 0f : 90f)) {
            b.InterpolationColors = new ColorBlend { Colors = new[] { PlasticDark, PlasticHi, Plastic, PlasticDark }, Positions = new[] { 0f, 0.28f, 0.62f, 1f } };
            g.FillPath(b, path);
            using (var pen = new Pen(Color.FromArgb(160, 0, 0, 0), 2)) g.DrawPath(pen, path);
        }
    }
    static void PrintAt(Graphics g, string s, Font f, Color c, float x, float y, float w, StringAlignment a) {
        Write(g, s, f, c, new RectangleF(x, y, w, f.Height * 1.3f), a, StringAlignment.Near, false);
    }
    // A key on the bezel: lighter while the mouse is over it.
    void HardKey(Graphics g, RectangleF r, string key, string label, float rad) {
        bool over = hoverPrompt == key;
        Card(g, r, rad, over ? Color.FromArgb(132, 136, 146) : Color.FromArgb(92, 95, 104), over ? Color.FromArgb(70, 72, 80) : Color.FromArgb(44, 46, 52), Color.FromArgb(170, 0, 0, 0), 2);
        if (label != null) Write(g, label, UKey, Print, r, StringAlignment.Center, StringAlignment.Center, false);
        Key(r, key);
    }
    // The hinge barrel on the right edge: where the panel meets the camera body (out of view).
    static void Hinge(Graphics g) {
        Cylinder(g, new RectangleF(1186, 150, 38, 460), 16, true);
        using (var pen = new Pen(Color.FromArgb(110, 0, 0, 0), 2)) for (float y = 200; y < 600; y += 70) g.DrawLine(pen, 1188, y, 1222, y);
    }

    // The panel facing you: the moulded frame with a raised lip, the dark gasket round the glass, the screen, and the
    // bezel: the backlight sensor on top, HOME, the T / W zoom rocker, SET, START/STOP and POWER down the
    // left, the speaker grille, print, DISPLAY, the tape keys and the model print along the bottom; then its wear and\n    // its sticker.
    void LcdPanel(Graphics g, float level, float t, bool live) {
        using (var path = Round(PanelR, 34)) {
            using (var b = new LinearGradientBrush(Grow(PanelR, 1), Color.FromArgb(76, 78, 86), Color.FromArgb(32, 33, 38), 90f)) g.FillPath(b, path);
            using (var pen = new Pen(Color.FromArgb(210, 0, 0, 0), 3)) g.DrawPath(pen, path);
        }
        using (var path = Round(Grow(PanelR, -10), 26)) using (var pen = new Pen(Color.FromArgb(30, 255, 255, 255), 2)) g.DrawPath(pen, path);
        PrintAt(g, "SK8-1000  HD", UModel, Print, 822, 700, 272, StringAlignment.Far);
        using (var b = new SolidBrush(Color.FromArgb(14, 14, 16))) g.FillEllipse(b, 1144, 30, 12, 12);
        using (var path = Round(Grow(ScreenR, 13), 10)) using (var b = new SolidBrush(Color.FromArgb(8, 8, 10))) g.FillPath(b, path);
        if (level > 0) {
            // The canvas is the screen's own pixels: copied 1:1 at whole-pixel coordinates (a half-pixel offset read a row
            // and column from outside it, a flickering edge).
            var lcd = LcdCanvas(t);
            var st = g.Save();
            var at = new PointF[] { new PointF(ScreenR.X, ScreenR.Y) };
            g.TransformPoints(CoordinateSpace.Device, CoordinateSpace.World, at);
            g.ResetTransform();
            g.DrawImageUnscaled(lcd, (int)Math.Round(at[0].X), (int)Math.Round(at[0].Y));
            g.Restore(st);
            if (level < 1) using (var b = new SolidBrush(Color.FromArgb((int)(255 * (1 - level)), 6, 7, 9))) g.FillRectangle(b, ScreenR);
        } else using (var b = new SolidBrush(Color.FromArgb(16, 17, 20))) g.FillRectangle(b, ScreenR);
        using (var b = new SolidBrush(Color.FromArgb(14, 255, 255, 255))) g.FillPolygon(b, new[] { new PointF(ScreenR.X, ScreenR.Y), new PointF(ScreenR.X + ScreenR.Width * 0.42f, ScreenR.Y), new PointF(ScreenR.X, ScreenR.Y + ScreenR.Height * 0.55f) });
        // Left strip: HOME (back), the zoom rocker (T up, W down), SET (select), START/STOP (play), POWER (close).
        HardKey(g, new RectangleF(40, 70, 110, 48), "MENU", "HOME", 12);
        PrintAt(g, "ZOOM", UTiny, Print, 30, 146, 130, StringAlignment.Center);
        var rocker = new RectangleF(66, 168, 58, 172);
        Card(g, rocker, 29, Color.FromArgb(92, 95, 104), Color.FromArgb(44, 46, 52), Color.FromArgb(170, 0, 0, 0), 2);
        var tHalf = new RectangleF(rocker.X, rocker.Y, rocker.Width, rocker.Height / 2); var wHalf = new RectangleF(rocker.X, rocker.Y + rocker.Height / 2, rocker.Width, rocker.Height / 2);
        if (hoverPrompt == "U") Card(g, tHalf, 29, Color.FromArgb(70, 255, 255, 255), Color.FromArgb(30, 255, 255, 255), Color.Empty, 0);
        if (hoverPrompt == "D") Card(g, wHalf, 29, Color.FromArgb(70, 255, 255, 255), Color.FromArgb(30, 255, 255, 255), Color.Empty, 0);
        using (var pen = new Pen(Color.FromArgb(150, 0, 0, 0), 2)) g.DrawLine(pen, rocker.X + 8, rocker.Y + rocker.Height / 2, rocker.Right - 8, rocker.Y + rocker.Height / 2);
        Write(g, "T", UKey, Print, tHalf, StringAlignment.Center, StringAlignment.Center, false);
        Write(g, "W", UKey, Print, wHalf, StringAlignment.Center, StringAlignment.Center, false);
        Key(tHalf, "U"); Key(wHalf, "D");
        HardKey(g, new RectangleF(40, 372, 110, 48), "A", "SET", 12);
        PrintAt(g, "START/STOP", UTiny, Print, 30, 436, 130, StringAlignment.Center);
        using (var b = new SolidBrush(Color.FromArgb(26, 27, 31))) g.FillEllipse(b, 39, 456, 112, 112);
        bool overPlay = hoverPrompt == "PLAY";
        using (var b = new LinearGradientBrush(new RectangleF(51, 468, 88, 88), overPlay ? Color.FromArgb(255, 120, 110) : Color.FromArgb(240, 70, 60), Color.FromArgb(150, 20, 16), 90f)) g.FillEllipse(b, 51, 468, 88, 88);
        using (var b = new SolidBrush(Color.FromArgb(70, 255, 255, 255))) g.FillEllipse(b, 70, 478, 36, 20);
        Key(new RectangleF(39, 456, 112, 112), "PLAY");
        HardKey(g, new RectangleF(48, 588, 94, 36), "POWER", null, 10);
        Write(g, "POWER", UTiny, Print, new RectangleF(48, 588, 94, 36), StringAlignment.Center, StringAlignment.Center, false);
        // Bottom strip: speaker grille, print, DISPLAY and the tape keys (rewind, play, fast forward, stop).
        using (var b = new SolidBrush(Color.FromArgb(150, 0, 0, 0))) for (int r = 0; r < 4; r++) for (int c = 0; c < 7; c++) g.FillEllipse(b, 188 + c * 12 + (r % 2) * 6, 642 + r * 12, 6, 6);
        PrintAt(g, "16:9 WIDE LCD", UTiny, Print, 180, 700, 160, StringAlignment.Near);
        HardKey(g, new RectangleF(716, 640, 84, 32), "DISPLAY", null, 8);
        PrintAt(g, "DISPLAY", UTiny, Print, 716, 676, 84, StringAlignment.Center);
        string[] keys = { "L", "A", "R", "MENU" }, names = { "REW", "PLAY", "FF", "STOP" };
        for (int i = 0; i < 4; i++) {
            var r = new RectangleF(826 + i * 66, 640, 56, 32);
            HardKey(g, r, keys[i], null, 8);
            TapeGlyph(g, i, r);
            PrintAt(g, names[i], UTiny, Print, r.X - 6, 676, r.Width + 12, StringAlignment.Center);
        }
        DrawMarks(g);
        PlaceSticker(g, StickerSpec());
    }
    static void TapeGlyph(Graphics g, int kind, RectangleF r) {
        float cx = r.X + r.Width / 2, cy = r.Y + r.Height / 2, s = 8;
        using (var b = new SolidBrush(Print)) {
            if (kind == 0) { g.FillPolygon(b, new[] { new PointF(cx - 1, cy), new PointF(cx + 9, cy - s), new PointF(cx + 9, cy + s) }); g.FillPolygon(b, new[] { new PointF(cx - 11, cy), new PointF(cx - 1, cy - s), new PointF(cx - 1, cy + s) }); }
            if (kind == 1) g.FillPolygon(b, new[] { new PointF(cx + 9, cy), new PointF(cx - 6, cy - s), new PointF(cx - 6, cy + s) });
            if (kind == 2) { g.FillPolygon(b, new[] { new PointF(cx + 1, cy), new PointF(cx - 9, cy - s), new PointF(cx - 9, cy + s) }); g.FillPolygon(b, new[] { new PointF(cx + 11, cy), new PointF(cx + 1, cy - s), new PointF(cx + 1, cy + s) }); }
            if (kind == 3) g.FillRectangle(b, cx - 7, cy - 7, 14, 14);
        }
    }
    // Mid-swing: the panel (screen still dark) drawn into a bitmap once and swung on the hinge at its right edge, in
    // vertical strips with perspective: the free edge recedes while it is edge-on, and comes a touch closer during the
    // wobble past flat; darker while it is turned away.
    Bitmap flipScreen; Bitmap[] screenStrips; RectangleF[] screenInner; float flipScale = -1;
    const int Strips = 48;
    Bitmap PanelShot(float t) {
        var shot = new Bitmap((int)Math.Ceiling(PanelR.Width * devS) + 2, (int)Math.Ceiling(PanelR.Height * devS) + 2, PixelFormat.Format32bppPArgb);
        var keep = new List<KeyValuePair<RectangleF, string>>(promptHits);
        using (var pg = Graphics.FromImage(shot)) {
            pg.SmoothingMode = SmoothingMode.AntiAlias; pg.TextRenderingHint = System.Drawing.Text.TextRenderingHint.AntiAliasGridFit;
            pg.ScaleTransform(devS, devS); pg.TranslateTransform(-PanelR.X, -PanelR.Y);
            LcdPanel(pg, 0, t, false);
        }
        promptHits.Clear(); promptHits.AddRange(keep);
        return shot;
    }
    // A face cut into its vertical strips once, so each strip draws on its own (GDI+ prepares a whole image per call).
    // Each strip carries one pixel of its neighbours on both sides, drawn over them, so the smoothing at its edges
    // blends with real pixels and the strips meet without seams.
    static Bitmap[] Slice(Bitmap shot, out RectangleF[] inner) {
        var parts = new Bitmap[Strips]; inner = new RectangleF[Strips];
        for (int i = 0; i < Strips; i++) {
            int sx = i * shot.Width / Strips, sw = (i + 1) * shot.Width / Strips - sx;
            int x0 = Math.Max(0, sx - 1), x1 = Math.Min(shot.Width, sx + sw + 1);
            parts[i] = shot.Clone(new Rectangle(x0, 0, x1 - x0, shot.Height), PixelFormat.Format32bppPArgb);
            inner[i] = new RectangleF(sx - x0, 0, sw, shot.Height);
        }
        return parts;
    }
    // The panel's face is drawn once per size, on the first (still invisible) frame: only the swing changes per frame.
    void EnsureFlip(float t) {
        if (flipScale == devS) return;
        if (flipScreen != null) flipScreen.Dispose();
        if (screenStrips != null) foreach (var s in screenStrips) s.Dispose();
        flipScreen = PanelShot(t); flipScale = devS;
        screenStrips = Slice(flipScreen, out screenInner);
    }
    void FlipPanel(Graphics g, float open, float t) {
        if (open <= 0.01f) return;
        double a = (1 - open) * Math.PI / 2;
        float cos = (float)Math.Cos(a), sin = (float)Math.Sin(a), W = PanelR.Width, H = PanelR.Height, cy = PanelR.Y + H / 2, hx = PanelR.Right, dist = W * 2f;
        Func<float, float> scale = delegate(float u) { return dist / (dist + 0.6f * u * W * sin); };
        Func<float, float> xAt = delegate(float u) { return hx - u * W * cos * scale(u); };
        var keepInterp = g.InterpolationMode; var keepOffset = g.PixelOffsetMode; var keepMode = g.CompositingQuality;
        g.InterpolationMode = InterpolationMode.NearestNeighbor; g.PixelOffsetMode = PixelOffsetMode.HighSpeed; g.CompositingQuality = CompositingQuality.HighSpeed;
        for (int i = 0; i < Strips; i++) {
            float uL = 1 - (float)i / Strips, uR = 1 - (i + 1f) / Strips, sm = scale((uL + uR) / 2);
            float xl = xAt(uL), xr = xAt(uR), top = cy - H / 2 * sm;
            if (xr - xl < 0.05f) continue;
            float kx = (xr - xl) / screenInner[i].Width;
            g.DrawImage(screenStrips[i], new RectangleF(xl - screenInner[i].X * kx, top, screenStrips[i].Width * kx, H * sm));
        }
        g.InterpolationMode = keepInterp; g.PixelOffsetMode = keepOffset; g.CompositingQuality = keepMode;
        int dark = (int)(170 * Math.Max(0, sin));
        if (dark > 0) using (var b = new SolidBrush(Color.FromArgb(dark, 0, 0, 0))) {
            float s1 = scale(1), x1 = xAt(1);
            g.FillPolygon(b, new[] { new PointF(x1, cy - H / 2 * s1), new PointF(hx, PanelR.Y), new PointF(hx, PanelR.Bottom), new PointF(x1, cy + H / 2 * s1) });
        }
    }

    // ---------- the panel's wear (USER-ASKS 489) ----------
    // Rolled fresh at every start: paint-marker scrawl in skate-session words, doodles, scratch clusters, paint rubbed
    // off along the edges, now and then gaff tape across a bottom corner. All of it stays on the wear map: the bezel
    // minus keep-out zones over every UI element on it (glass, keys, labels, print, grille, sensor), so wear never
    // covers the panel's UI. Tags and doodles shrink until their whole outline fits the map.
    class Mark { public int Spot, Kind, Seed; public float X, Y, Angle, Size; public string Text; public Color Ink; public bool Placed; }
    readonly List<Mark> marks = new List<Mark>();
    static readonly string[] Scrawl = { "ONE MORE TRY", "LAST TRY", "FILM IT", "SEND IT", "SESH", "DIY OR DIE", "SKATE & ANNOY", "NO COMPLY", "KICKFLIP?", "SECURITY!!", "SK8", "LINE!!", "HOMIES", "RAD", "BUST", "FILMER", "1 MORE", "SLAMS", "GNAR", "POP!", "SK8!" };
    static readonly Color[] Markers = { Color.FromArgb(232, 246, 246, 240), Color.FromArgb(220, 204, 208, 214), Color.FromArgb(225, 236, 64, 54), Color.FromArgb(225, 250, 214, 60), Color.FromArgb(225, 250, 112, 172), Color.FromArgb(225, 96, 212, 232) };
    // Spots for tags and doodles: the bottom strip between the print and DISPLAY, the left strip under POWER (the top
    // strip between the frame and the glass is too thin for a word).
    static readonly RectangleF[] TagAreas = { new RectangleF(352, 630, 348, 104), new RectangleF(36, 640, 120, 90) };
    static readonly int[] TagChars = { 12, 4 };
    static readonly float[] TagTilt = { 5, 8 };
    static readonly RectangleF[] KeepOut = {
        Grow(ScreenR, 15), new RectangleF(950, 696, 150, 30), new RectangleF(1136, 22, 30, 28),
        new RectangleF(34, 64, 122, 60), new RectangleF(30, 140, 130, 210), new RectangleF(34, 366, 122, 60),
        new RectangleF(30, 430, 130, 144), new RectangleF(42, 582, 106, 48), new RectangleF(180, 634, 104, 64),
        new RectangleF(176, 696, 170, 28), new RectangleF(710, 634, 96, 64), new RectangleF(820, 634, 272, 64),
    };
    static readonly FontFamily Hand = HandFamily();
    static FontFamily HandFamily() {
        foreach (var name in new[] { "Ink Free", "Segoe Print", "Comic Sans MS" }) foreach (var fam in FontFamily.Families) if (fam.Name == name) return fam;
        return FontFamily.GenericSansSerif;
    }
    static string PickWord(Random r, int max) {
        var fit = Array.FindAll(Scrawl, w => w.Length <= max);
        return fit[r.Next(fit.Length)];
    }
    void RollMarks(Random r) {
        marks.Clear();
        Func<double, double, float> u = delegate(double lo, double hi) { return (float)(lo + r.NextDouble() * (hi - lo)); };
        var free = new List<int> { 0, 1 };
        int tags = 1 + r.Next(2);
        for (int i = 0; i < tags; i++) {
            int spot = free[r.Next(free.Count)]; free.Remove(spot);
            var a = TagAreas[spot];
            marks.Add(new Mark { Spot = spot, Kind = 0, X = a.X + a.Width * u(0.35, 0.65), Y = a.Y + a.Height * u(0.4, 0.6), Angle = TagTilt[spot] * u(-1, 1), Text = PickWord(r, TagChars[spot]), Ink = Markers[r.Next(Markers.Length)], Seed = r.Next() });
        }
        int doodles = 1 + r.Next(2);
        for (int i = 0; i < doodles; i++) {
            int spot = r.Next(2);
            var a = TagAreas[spot];
            marks.Add(new Mark { Spot = spot, Kind = 1, X = a.X + a.Width * u(0.1, 0.9), Y = a.Y + a.Height * u(0.3, 0.7), Angle = u(-20, 20), Size = Math.Min(a.Height * 0.7f, 46) * u(0.7, 1), Ink = Markers[r.Next(Markers.Length)], Seed = r.Next(5) });
        }
        int scratches = 3 + r.Next(4);
        for (int i = 0; i < scratches; i++)
            marks.Add(new Mark { Kind = 2, X = u(PanelR.X + 30, PanelR.Right - 40), Y = r.Next(2) == 0 ? u(PanelR.Y + 6, ScreenR.Y - 14) : u(ScreenR.Bottom + 16, PanelR.Bottom - 10), Angle = u(-30, 30), Size = u(30, 90), Seed = r.Next() });
        int edges = 1 + r.Next(2);
        for (int i = 0; i < edges; i++) {
            int side = r.Next(3);
            marks.Add(new Mark { Kind = 3, X = side == 2 ? PanelR.X + 3 : u(PanelR.X + 60, PanelR.Right - 340), Y = side == 0 ? PanelR.Y + 3 : side == 1 ? PanelR.Bottom - 3 : u(PanelR.Y + 60, PanelR.Bottom - 340), Angle = side == 2 ? 90 : 0, Size = u(120, 300), Seed = r.Next() });
        }
        if (r.Next(2) == 0) {
            bool left = r.Next(2) == 0;
            marks.Add(new Mark { Kind = 4, X = left ? PanelR.X + 26 : PanelR.Right - 26, Y = PanelR.Bottom - 26, Angle = (left ? 45 : -45) + u(-8, 8), Size = u(80, 110), Seed = r.Next() });
        }
    }
    // The panel's outline, and the wear map: the outline minus every keep-out zone.
    static GraphicsPath PartOutline() { return Round(PanelR, 34); }
    static Region WearMap() {
        var r = new Region(PartOutline());
        foreach (var k in KeepOut) r.Exclude(k);
        return r;
    }
    // Where tags and doodles may go: the wear map minus the sticker, so a word never ends up half under it.
    Region TagMap() {
        var r = WearMap();
        var at = StickerAt(StickerSpec());
        using (var foot = new GraphicsPath()) { foot.AddEllipse(at.X - StickerReach, at.Y - StickerReach, 2 * StickerReach, 2 * StickerReach); r.Exclude(foot); }
        return r;
    }
    // The bottom spot shares the strip with the sticker: a tag or doodle there moves to the side with more room.
    RectangleF SpotArea(int spot) {
        var a = TagAreas[spot];
        if (spot != 0) return a;
        float sx = StickerAt(StickerSpec()).X, left = sx - StickerReach - a.X, right = a.Right - (sx + StickerReach);
        return left >= right ? new RectangleF(a.X, a.Y, Math.Max(20, left), a.Height) : new RectangleF(sx + StickerReach, a.Y, Math.Max(20, right), a.Height);
    }
    int notFit;   // tags / doodles that could not shrink clear of the sticker and the keep-outs (--wear-test fails on any)
    bool marksUnclipped;   // --wear-test: tags, doodles and tape drawn without the keep-outs, to find any that would cover UI
    void DrawMarks(Graphics g) {
        foreach (var mk in marks) {
            if (!mk.Placed && (mk.Kind == 0 || mk.Kind == 1)) { var a = SpotArea(mk.Spot); mk.X = a.X + a.Width / 2 + (mk.X - (TagAreas[mk.Spot].X + TagAreas[mk.Spot].Width / 2)) * a.Width / TagAreas[mk.Spot].Width; mk.Placed = true; }
            var rnd = new Random(mk.Seed);
            var ms = g.Save();
            if (!marksUnclipped || mk.Kind == 2 || mk.Kind == 3) using (var clip = WearMap()) g.SetClip(clip, CombineMode.Intersect);
            else using (var clip = PartOutline()) g.SetClip(clip, CombineMode.Intersect);
            g.TranslateTransform(mk.X, mk.Y); g.RotateTransform(mk.Angle);
            if (mk.Kind == 0) MarkerTag(g, mk, rnd);
            else if (mk.Kind == 1) Doodle(g, mk, rnd);
            else if (mk.Kind == 2) using (var lite = new Pen(Color.FromArgb(70, 210, 214, 220), 1.1f)) using (var dark = new Pen(Color.FromArgb(50, 0, 0, 0), 1.1f)) {
                int n = 5 + rnd.Next(8);
                for (int i = 0; i < n; i++) {
                    float x0 = (float)(rnd.NextDouble() * mk.Size - mk.Size / 2), y0 = (float)(rnd.NextDouble() * 18 - 9), len = (float)(rnd.NextDouble() * mk.Size * 0.6 + 8), dy = (float)(rnd.NextDouble() * 6 - 3);
                    g.DrawLine(dark, x0, y0 + 1, x0 + len, y0 + dy + 1); g.DrawLine(lite, x0, y0, x0 + len, y0 + dy);
                }
            }
            else if (mk.Kind == 3) using (var rub = new Pen(Color.FromArgb(120, 128, 131, 140), 3)) {
                for (int i = 0; i < 26; i++) { float x0 = (float)(rnd.NextDouble() * mk.Size), len = (float)(rnd.NextDouble() * 26 + 4), off = (float)(rnd.NextDouble() * 6 - 3); rub.Width = (float)(rnd.NextDouble() * 3 + 1); g.DrawLine(rub, x0, off, x0 + len, off + (float)(rnd.NextDouble() * 2 - 1)); }
            }
            else if (mk.Kind == 4) GaffTape(g, mk, rnd);
            g.Restore(ms);
        }
    }
    // Whether a mark's outline, widened by its pen and smoothing, lies wholly on the wear map.
    static bool Inside(Region map, GraphicsPath local, Mark mk) {
        using (var test = (GraphicsPath)local.Clone()) using (var m = new Matrix()) using (var pen = new Pen(Color.Black, 5)) {
            m.Translate(mk.X, mk.Y); m.Rotate(mk.Angle); test.Transform(m); test.Widen(pen); test.Flatten();
            foreach (var pt in test.PathPoints) if (!map.IsVisible(pt)) return false;
        }
        return true;
    }
    // Paint-marker scrawl: the word in a hand font, sized to its spot (and shrunk until it fits the wear map), with the
    // paint worn through in places.
    void MarkerTag(Graphics g, Mark mk, Random rnd) {
        var area = SpotArea(mk.Spot);
        using (var path = new GraphicsPath()) {
            path.AddString(mk.Text, Hand, (int)FontStyle.Bold, area.Height * 0.7f, PointF.Empty, StringFormat.GenericTypographic);
            var b = path.GetBounds();
            float k = Math.Min(1, area.Width * 0.85f / Math.Max(1, b.Width));
            using (var m = new Matrix()) { m.Scale(k, k); m.Translate(-b.X - b.Width / 2, -b.Y - b.Height / 2); path.Transform(m); }
            using (var map = TagMap())
            {
                for (int tries = 0; tries < 12 && !Inside(map, path, mk); tries++) using (var m = new Matrix()) { m.Scale(0.9f, 0.9f); path.Transform(m); }
                if (!Inside(map, path, mk)) notFit++;
            }
            using (var br = new SolidBrush(mk.Ink)) g.FillPath(br, path);
            using (var pen = new Pen(Fade(mk.Ink, 120), 1.2f)) g.DrawPath(pen, path);
            var kept = g.Save();
            g.SetClip(path, CombineMode.Intersect);
            var nb = path.GetBounds();
            using (var wear = new SolidBrush(Color.FromArgb(170, 50, 52, 58)))
                for (int i = 0; i < 60; i++) g.FillEllipse(wear, nb.X + (float)rnd.NextDouble() * nb.Width, nb.Y + (float)rnd.NextDouble() * nb.Height, 1.5f + (float)rnd.NextDouble() * 3, 1 + (float)rnd.NextDouble() * 2);
            using (var streak = new Pen(Color.FromArgb(90, 50, 52, 58), 1.6f))
                for (int i = 0; i < 5; i++) { float y = nb.Y + (float)rnd.NextDouble() * nb.Height; g.DrawLine(streak, nb.X, y, nb.Right, y + (float)(rnd.NextDouble() * 4 - 2)); }
            g.Restore(kept);
        }
    }
    // A quick marker doodle: crown, star, arrow, X or smiley (shrunk until it fits the wear map, pen width included).
    void Doodle(Graphics g, Mark mk, Random rnd) {
        float s = mk.Size / 2;
        using (var map = TagMap())
            for (int tries = 0; tries < 12; tries++) {
                using (var ring = new GraphicsPath()) { float rr = s * 1.15f + 3; ring.AddEllipse(-rr, -rr, 2 * rr, 2 * rr); if (Inside(map, ring, mk)) break; }
                s *= 0.88f;
                if (tries == 11) notFit++;
            }
        using (var pen = new Pen(mk.Ink, Math.Max(2.4f, s * 0.12f))) {
            pen.LineJoin = LineJoin.Round; pen.StartCap = LineCap.Round; pen.EndCap = LineCap.Round;
            if (mk.Seed == 0) Wobble(g, pen, rnd, new PointF(-s, s * 0.6f), new PointF(-s, -s * 0.5f), new PointF(-s * 0.5f, s * 0.05f), new PointF(0, -s * 0.8f), new PointF(s * 0.5f, s * 0.05f), new PointF(s, -s * 0.5f), new PointF(s, s * 0.6f), new PointF(-s, s * 0.6f));
            else if (mk.Seed == 1) { var pts = new PointF[11]; for (int i = 0; i <= 10; i++) { double a = -Math.PI / 2 + i * Math.PI / 5; float rr = i % 2 == 0 ? s : s * 0.42f; pts[i] = new PointF((float)Math.Cos(a) * rr, (float)Math.Sin(a) * rr); } Wobble(g, pen, rnd, pts); }
            else if (mk.Seed == 2) { Wobble(g, pen, rnd, new PointF(-s, s * 0.2f), new PointF(-s * 0.2f, -s * 0.1f), new PointF(s, 0)); Wobble(g, pen, rnd, new PointF(s * 0.55f, -s * 0.45f), new PointF(s, 0), new PointF(s * 0.5f, s * 0.4f)); }
            else if (mk.Seed == 3) { Wobble(g, pen, rnd, new PointF(-s * 0.7f, -s * 0.7f), new PointF(0, 0), new PointF(s * 0.7f, s * 0.7f)); Wobble(g, pen, rnd, new PointF(s * 0.7f, -s * 0.7f), new PointF(0, 0), new PointF(-s * 0.7f, s * 0.7f)); }
            else { g.DrawEllipse(pen, -s, -s, 2 * s, 2 * s); g.DrawArc(pen, -s * 0.55f, -s * 0.3f, s * 1.1f, s * 0.95f, 20, 140); g.DrawLine(pen, -s * 0.35f, -s * 0.35f, -s * 0.35f, -s * 0.15f); g.DrawLine(pen, s * 0.35f, -s * 0.35f, s * 0.35f, -s * 0.15f); }
        }
    }
    // A strip of matte gaff tape across a corner, torn at both ends, a little wrinkled (it wraps round the panel's edge).
    static void GaffTape(Graphics g, Mark mk, Random rnd) {
        float w = mk.Size, h = 46;
        var pts = new List<PointF>();
        for (int i = 0; i <= 8; i++) pts.Add(new PointF(-w / 2 + (float)(rnd.NextDouble() * 5 - 2.5), -h / 2 + i * h / 8));
        for (int i = 8; i >= 0; i--) pts.Add(new PointF(w / 2 + (float)(rnd.NextDouble() * 5 - 2.5), -h / 2 + i * h / 8));
        using (var b = new SolidBrush(Color.FromArgb(90, 0, 0, 0))) { var sh = pts.ConvertAll(p => new PointF(p.X + 2, p.Y + 2)); g.FillPolygon(b, sh.ToArray()); }
        using (var b = new LinearGradientBrush(new RectangleF(-w / 2 - 4, -h / 2 - 1, w + 8, h + 2), Color.FromArgb(78, 78, 82), Color.FromArgb(34, 34, 38), 90f)) g.FillPolygon(b, pts.ToArray());
        using (var pen = new Pen(Color.FromArgb(50, 255, 255, 255), 1)) for (int i = 0; i < 4; i++) { float x = (float)(rnd.NextDouble() * w - w / 2); g.DrawLine(pen, x, -h / 2 + 4, x + (float)(rnd.NextDouble() * 10 - 5), h / 2 - 4); }
        using (var pen = new Pen(Color.FromArgb(40, 255, 255, 255), 2)) g.DrawLine(pen, -w / 2 + 4, -h / 2 + 8, w / 2 - 4, -h / 2 + 8);
    }
    // The ridges and edges under the sticker (the gasket edge and the bezel's inner lip), so it shows creases where it
    // was pressed over them.
    static List<PointF[]> Ridges() {
        return new List<PointF[]> {
            new[] { new PointF(PanelR.X, ScreenR.Bottom + 13), new PointF(PanelR.Right, ScreenR.Bottom + 13) },
            new[] { new PointF(PanelR.X, PanelR.Bottom - 10), new PointF(PanelR.Right, PanelR.Bottom - 10) },
        };
    }
    static void Creases(Graphics g, List<PointF[]> ridges) {
        using (var dark = new Pen(Color.FromArgb(100, 0, 0, 0), 2.4f)) using (var lite = new Pen(Color.FromArgb(90, 255, 255, 255), 1.5f))
            foreach (var seg in ridges) {
                float ex = seg[1].X - seg[0].X, ey = seg[1].Y - seg[0].Y, el = (float)Math.Sqrt(ex * ex + ey * ey);
                if (el < 1) continue;
                float ox = -ey / el * 1.8f, oy = ex / el * 1.8f;
                g.DrawLine(dark, seg[0], seg[1]);
                g.DrawLine(lite, seg[0].X + ox, seg[0].Y + oy, seg[1].X + ox, seg[1].Y + oy);
            }
    }

    // ---------- the sticker (USER-ASKS 487) ----------
    // One sticker slapped on the camera years ago, picked once per user at the first start and kept in
    // camcorder-sticker.txt in the state folder: design, zone (0 bezel, 1 lens, 2 body print), place, angle, wear seed.
    // Placeholders: PNGs in the state folder's stickers\ (the game's own stickers, once the asset export finds them:
    // .claude/todo/living-world-stickers.md) replace the parody set below.
    const int StickerCount = 5;
    float[] sticker; Bitmap[] gameStickers;
    float[] StickerSpec() {
        if (sticker != null) return sticker;
        var inv = System.Globalization.CultureInfo.InvariantCulture;
        string f = Path.Combine(StateDir(), "camcorder-sticker.txt");
        if (!snapshotMode) try {
            if (File.Exists(f)) {
                var parts = File.ReadAllText(f).Trim().Split(' ');
                if (parts.Length == 6) return sticker = Array.ConvertAll(parts, x => float.Parse(x, inv));
            }
        } catch { }
        var rnd = snapshotMode ? new Random(snapSeed) : new Random();
        sticker = new float[] { rnd.Next(StickerCount), rnd.Next(3), (float)rnd.NextDouble(), (float)rnd.NextDouble(), (float)(rnd.NextDouble() * 40 - 20), rnd.Next(100000) };
        if (!snapshotMode) try { Directory.CreateDirectory(StateDir()); File.WriteAllText(f, string.Join(" ", Array.ConvertAll(sticker, x => x.ToString(inv)))); } catch { }
        return sticker;
    }
    Bitmap GameSticker(int pick) {
        if (gameStickers == null) {
            var list = new List<Bitmap>();
            try { string dir = Path.Combine(StateDir(), "stickers"); if (Directory.Exists(dir)) foreach (var p in Directory.GetFiles(dir, "*.png")) try { list.Add(new Bitmap(p)); } catch { } } catch { }
            gameStickers = list.ToArray();
        }
        return gameStickers.Length == 0 ? null : gameStickers[pick % gameStickers.Length];
    }
    // On the bezel's bottom strip, between the print and DISPLAY.
    static PointF StickerAt(float[] s) { return new PointF(420 + s[2] * 200, 682 + s[3] * 4); }
    const float StickerReach = 70;   // the sticker's footprint round its centre, kept clear of tags and doodles
    void PlaceSticker(Graphics g, float[] s) {
        float cx = StickerAt(s).X, cy = StickerAt(s).Y, scale = 0.68f;
        // The surface's ridges in the sticker's own space (the inverse of its placement).
        double ra = -s[4] * Math.PI / 180; float rc = (float)Math.Cos(ra), rs = (float)Math.Sin(ra);
        var local = Ridges().ConvertAll(seg => Array.ConvertAll(seg, q => { float dx = q.X - cx, dy = q.Y - cy; return new PointF((dx * rc - dy * rs) / scale, (dx * rs + dy * rc) / scale); }));
        var st = g.Save();
        g.TranslateTransform(cx, cy); g.RotateTransform(s[4]); g.ScaleTransform(scale, scale);
        var game = GameSticker((int)s[5]);
        if (game != null) {
            float k = 160f / Math.Max(game.Width, game.Height);
            var rect = new RectangleF(-game.Width * k / 2, -game.Height * k / 2, game.Width * k, game.Height * k);
            g.DrawImage(game, rect);
            var cs = g.Save(); g.SetClip(rect, CombineMode.Intersect); Creases(g, local); g.Restore(cs);
        } else DrawSticker(g, (int)s[0], (int)s[5], local);
        g.Restore(st);
    }
    // A parody sticker around (0, 0), about 160 x 160 at most, aged: faded, scuffed, chipped, one edge peeling up.
    static void DrawSticker(Graphics g, int design, int seed, List<PointF[]> ridges) {
        var rnd = new Random(seed);
        using (var shape = StickerShape(design)) {
            // The peel: the part past a line near one edge has lifted and folded back over.
            var bounds = shape.GetBounds();
            double pa = rnd.NextDouble() * Math.PI * 2;
            float nx = (float)Math.Cos(pa), ny = (float)Math.Sin(pa);
            float reach = Math.Abs(nx) * bounds.Width / 2 + Math.Abs(ny) * bounds.Height / 2;
            float dist = reach * (0.74f + 0.14f * (float)rnd.NextDouble());
            var p = new PointF(nx * dist, ny * dist);
            var dx = -ny; var dy = nx;
            using (var half = new GraphicsPath()) {
                half.AddPolygon(new[] { new PointF(p.X + dx * 400, p.Y + dy * 400), new PointF(p.X - dx * 400, p.Y - dy * 400), new PointF(p.X - dx * 400 + nx * 400, p.Y - dy * 400 + ny * 400), new PointF(p.X + dx * 400 + nx * 400, p.Y + dy * 400 + ny * 400) });
                using (var kept = new Region(shape)) using (var lifted = new Region(shape)) {
                    kept.Exclude(half); lifted.Intersect(half);
                    var gs = g.Save();
                    g.TranslateTransform(2, 3);
                    using (var b = new SolidBrush(Color.FromArgb(70, 0, 0, 0))) g.FillRegion(b, kept);
                    g.Restore(gs);
                    gs = g.Save();
                    g.SetClip(kept, CombineMode.Intersect);
                    StickerArt(g, design, rnd);
                    // Wear: scratches, grime, chipped bits along the edge.
                    using (var pen = new Pen(Color.FromArgb(110, 255, 255, 250), 1.2f)) for (int i = 0; i < 16; i++) { float x0 = (float)(rnd.NextDouble() * 160 - 80), y0 = (float)(rnd.NextDouble() * 160 - 80); g.DrawLine(pen, x0, y0, x0 + (float)(rnd.NextDouble() * 40 - 20), y0 + (float)(rnd.NextDouble() * 12 - 6)); }
                    using (var b = new SolidBrush(Color.FromArgb(60, 60, 50, 30))) for (int i = 0; i < 40; i++) g.FillEllipse(b, (float)(rnd.NextDouble() * 160 - 80), (float)(rnd.NextDouble() * 160 - 80), 2.5f, 2.5f);
                    using (var b = new SolidBrush(Color.FromArgb(30, 120, 110, 80))) g.FillRectangle(b, -90, -90, 180, 180);
                    Creases(g, ridges);
                    g.Restore(gs);
                    using (var pen = new Pen(Color.FromArgb(120, 40, 40, 44), 3)) {
                        for (int i = 0; i < 4; i++) {
                            double ca = rnd.NextDouble() * Math.PI * 2;
                            float ex = (float)Math.Cos(ca) * bounds.Width * 0.47f, ey = (float)Math.Sin(ca) * bounds.Height * 0.47f;
                            var cs = g.Save(); g.SetClip(kept, CombineMode.Intersect);
                            using (var b = new SolidBrush(Color.FromArgb(120, 46, 47, 52))) g.FillPolygon(b, new[] { new PointF(ex - 6, ey - 3), new PointF(ex + 5, ey - 5), new PointF(ex + 7, ey + 4), new PointF(ex - 3, ey + 7) });
                            g.Restore(cs);
                        }
                    }
                    // Old glue where it lifted, then the flap folded back over the line.
                    using (var b = new SolidBrush(Color.FromArgb(60, 210, 196, 150))) g.FillRegion(b, lifted);
                    float c2 = dx * dx - dy * dy, s2 = 2 * dx * dy;
                    var m = new Matrix(c2, s2, s2, -c2, p.X - (c2 * p.X + s2 * p.Y), p.Y - (s2 * p.X - c2 * p.Y));
                    gs = g.Save();
                    g.MultiplyTransform(m);
                    g.TranslateTransform(-nx * 3, -ny * 3);
                    using (var b = new SolidBrush(Color.FromArgb(80, 0, 0, 0))) g.FillRegion(b, lifted);
                    g.TranslateTransform(nx * 3, ny * 3);
                    using (var b = new LinearGradientBrush(new RectangleF(-90, -90, 180, 180), Color.FromArgb(238, 234, 222), Color.FromArgb(196, 190, 176), (float)(pa * 180 / Math.PI))) g.FillRegion(b, lifted);
                    g.Restore(gs);
                    gs = g.Save(); g.SetClip(shape, CombineMode.Intersect);
                    using (var pen = new Pen(Color.FromArgb(90, 0, 0, 0), 1.2f)) g.DrawLine(pen, p.X + dx * 120, p.Y + dy * 120, p.X - dx * 120, p.Y - dy * 120);
                    g.Restore(gs);
                }
            }
        }
    }
    static GraphicsPath StickerShape(int design) {
        var path = new GraphicsPath();
        if (design == 0) { path.AddClosedCurve(new[] { new PointF(-6, -80), new PointF(22, -64), new PointF(40, -10), new PointF(30, 54), new PointF(34, 80), new PointF(-30, 82), new PointF(-30, 30), new PointF(-42, -24), new PointF(-30, -62) }, 0.5f); return path; }
        if (design == 1) { path.AddArc(-80, -56, 160, 70, 180, 180); path.AddLine(80, -21, 80, 40); path.AddLine(80, 40, -80, 40); path.CloseFigure(); return path; }
        if (design == 2) { path.AddEllipse(-72, -72, 144, 144); return path; }
        if (design == 3) { path.AddRectangle(new RectangleF(-80, -30, 160, 60)); return path; }
        path.AddEllipse(-70, -70, 140, 140); return path;
    }
    // A wobbly hand-drawn line (the riso character sticker's look).
    static void Wobble(Graphics g, Pen pen, Random rnd, params PointF[] pts) {
        var w = new PointF[pts.Length];
        for (int i = 0; i < pts.Length; i++) w[i] = new PointF(pts[i].X + (float)(rnd.NextDouble() * 2.4 - 1.2), pts[i].Y + (float)(rnd.NextDouble() * 2.4 - 1.2));
        g.DrawCurve(pen, w, 0.5f);
    }
    static void FitText(Graphics g, string s, string family, FontStyle style, Color c, RectangleF r) {
        float size = r.Height;
        using (var probe = new Font(family, 100, style, GraphicsUnit.Pixel)) { var m = g.MeasureString(s, probe); size = Math.Min(r.Height * 100 / m.Height * 1.15f, r.Width * 100 / m.Width); }
        using (var f = new Font(family, Math.Max(4, size), style, GraphicsUnit.Pixel)) Write(g, s, f, c, new RectangleF(r.X - 20, r.Y - size * 0.1f, r.Width + 40, r.Height + size * 0.3f), StringAlignment.Center, StringAlignment.Center, false);
    }
    static void StickerArt(Graphics g, int design, Random rnd) {
        if (design == 0) {
            // "The filmer": an original character on clear vinyl, purple hand-drawn lines, flat fills printed off the lines.
            using (var b = new SolidBrush(Color.FromArgb(70, 255, 255, 255))) g.FillRectangle(b, -90, -90, 180, 180);
            var line = Color.FromArgb(84, 58, 160);
            Action<Color, PointF[]> fill = delegate(Color c, PointF[] pts) {
                var off = new PointF[pts.Length]; for (int i = 0; i < pts.Length; i++) off[i] = new PointF(pts[i].X + 2.5f, pts[i].Y + 1.5f);
                using (var b = new SolidBrush(c)) g.FillClosedCurve(b, off, FillMode.Alternate, 0.4f);
            };
            var head = new[] { new PointF(-14, -56), new PointF(-2, -66), new PointF(12, -58), new PointF(12, -40), new PointF(0, -32), new PointF(-12, -38) };
            var hoodie = new[] { new PointF(-22, -30), new PointF(18, -32), new PointF(26, 10), new PointF(20, 16), new PointF(-22, 16), new PointF(-28, 6) };
            var pants = new[] { new PointF(-22, 14), new PointF(20, 14), new PointF(24, 64), new PointF(4, 66), new PointF(0, 30), new PointF(-6, 66), new PointF(-26, 64) };
            var arm = new[] { new PointF(12, -26), new PointF(24, -40), new PointF(30, -56), new PointF(22, -60), new PointF(14, -46), new PointF(6, -30) };
            fill(Color.FromArgb(232, 196, 64), hoodie);
            fill(Color.FromArgb(150, 178, 214), pants);
            fill(Color.FromArgb(250, 240, 222), head);
            fill(Color.FromArgb(232, 196, 64), arm);
            using (var b = new SolidBrush(Color.FromArgb(170, 120, 170))) g.FillRectangle(b, -20, 6, 30, 12);
            using (var b = new SolidBrush(Color.FromArgb(150, 196, 80))) { g.FillEllipse(b, -30, 62, 26, 12); g.FillEllipse(b, 2, 62, 26, 12); }
            using (var b = new SolidBrush(Color.FromArgb(58, 58, 66))) g.FillRectangle(b, 14, -70, 30, 18);
            using (var b = new SolidBrush(Color.FromArgb(120, 150, 200))) g.FillEllipse(b, 38, -67, 10, 12);
            using (var pen = new Pen(line, 2.4f)) {
                pen.LineJoin = LineJoin.Round;
                Wobble(g, pen, rnd, head[0], head[1], head[2], head[3], head[4], head[5], head[0]);
                Wobble(g, pen, rnd, new PointF(-14, -56), new PointF(-4, -68), new PointF(10, -62), new PointF(16, -58));
                Wobble(g, pen, rnd, hoodie[0], hoodie[5], hoodie[4], hoodie[3], hoodie[2], hoodie[1]);
                Wobble(g, pen, rnd, pants[0], pants[6], pants[5], pants[4], pants[3], pants[2], pants[1]);
                Wobble(g, pen, rnd, arm[0], arm[1], arm[2], arm[3], arm[4], arm[5]);
                g.DrawRectangle(pen, 14, -70, 30, 18);
                Wobble(g, pen, rnd, new PointF(-2, -48), new PointF(2, -46), new PointF(4, -42));
                Wobble(g, pen, rnd, new PointF(-6, -30), new PointF(-4, 0), new PointF(-2, 12));
                g.DrawEllipse(pen, -30, 62, 26, 12); g.DrawEllipse(pen, 2, 62, 26, 12);
            }
            using (var pen = new Pen(Color.FromArgb(130, 255, 255, 255), 1.5f)) g.DrawPath(pen, StickerShape(0));
            return;
        }
        var cream = Color.FromArgb(240, 234, 218);
        using (var b = new SolidBrush(cream)) g.FillRectangle(b, -90, -90, 180, 180);
        if (design == 1) {
            // "BLICKEY": a workwear-tag parody, an arch-topped badge.
            using (var inner = new GraphicsPath()) {
                inner.AddArc(-72, -50, 144, 60, 180, 180); inner.AddLine(72, -20, 72, 33); inner.AddLine(72, 33, -72, 33); inner.CloseFigure();
                using (var b = new SolidBrush(Color.FromArgb(150, 132, 88))) g.FillPath(b, inner);
            }
            FitText(g, "BLICKEY", "Arial Black", FontStyle.Regular, cream, new RectangleF(-64, -20, 128, 30));
            FitText(g, "SINCE 1922 (PROBABLY)", "Tahoma", FontStyle.Bold, cream, new RectangleF(-56, 14, 112, 12));
        } else if (design == 2) {
            // "SPITBALL WHEELS": a round flame-head wheel badge parody.
            using (var b = new SolidBrush(Color.FromArgb(214, 52, 38))) g.FillEllipse(b, -64, -64, 128, 128);
            using (var b = new SolidBrush(Color.FromArgb(252, 196, 60)))
                for (int i = 0; i < 9; i++) { double a = -Math.PI / 2 + (i - 4) * 0.28; g.FillPolygon(b, new[] { new PointF((float)Math.Cos(a - 0.12) * 26, (float)Math.Sin(a - 0.12) * 26 - 6), new PointF((float)Math.Cos(a) * 54, (float)Math.Sin(a) * 54 - 6), new PointF((float)Math.Cos(a + 0.12) * 26, (float)Math.Sin(a + 0.12) * 26 - 6) }); }
            using (var b = new SolidBrush(cream)) g.FillEllipse(b, -24, -28, 48, 48);
            using (var pen = new Pen(Color.FromArgb(40, 30, 30), 3)) { g.DrawEllipse(pen, -24, -28, 48, 48); g.DrawArc(pen, -10, -6, 20, 14, 10, 160); }
            using (var b = new SolidBrush(Color.FromArgb(40, 30, 30))) { g.FillEllipse(b, -11, -14, 6, 8); g.FillEllipse(b, 5, -14, 6, 8); }
            FitText(g, "SPITBALL WHEELS", "Arial Black", FontStyle.Regular, cream, new RectangleF(-54, 28, 108, 22));
        } else if (design == 3) {
            // "SUPREMELY MID": the box-logo parody.
            using (var b = new SolidBrush(Color.FromArgb(222, 30, 36))) g.FillRectangle(b, -74, -24, 148, 48);
            FitText(g, "Supremely Mid", "Arial", FontStyle.Bold | FontStyle.Italic, Color.White, new RectangleF(-68, -18, 136, 36));
        } else {
            // "THRASHED": the flame-lettering magazine parody on a black disc.
            using (var b = new SolidBrush(Color.FromArgb(20, 20, 22))) g.FillEllipse(b, -62, -62, 124, 124);
            using (var b = new SolidBrush(Color.FromArgb(240, 110, 30)))
                for (int i = 0; i < 7; i++) { float x = -48 + i * 16; g.FillPolygon(b, new[] { new PointF(x - 7, 10), new PointF(x + 2, -30 - (i % 3) * 8), new PointF(x + 9, 10) }); }
            FitText(g, "THRASHED", "Impact", FontStyle.Regular, Color.FromArgb(255, 226, 120), new RectangleF(-54, -12, 108, 30));
            FitText(g, "SKATE AND REST", "Tahoma", FontStyle.Bold, cream, new RectangleF(-40, 22, 80, 12));
        }
    }

    // ---------- the LCD ----------
    // The screen of a mid-2000s HD camcorder (USER-ASKS 488): smooth type, glossy translucent rounded panels, an amber
    // selection bar with a sheen, the status row with its HDV badge, the board's lane and the date stamp. Drawn at the
    // screen's own pixels in a 340-wide layout space (lcdW x lcdH) over the cached skate-spot scene.
    Bitmap lcdBmp, sceneBmp; Font osdBig, osdRow, osdSmall, osdTiny, osdLogo; int osdFontH = -1;
    bool recOn; DateTime recSince; TimeSpan recTotal;
    int lcdW = 340, lcdH = 191;
    static readonly Color LcdInk = Color.FromArgb(244, 246, 250), LcdDim = Color.FromArgb(168, 178, 196), LcdAmberHi = Color.FromArgb(255, 198, 88),
        LcdAmberLo = Color.FromArgb(238, 124, 12), LcdAmber = Color.FromArgb(255, 172, 44), LcdGreen = Color.FromArgb(124, 220, 96), LcdRed = Color.FromArgb(255, 98, 86);
    void LoHit(RectangleF lo, int index) {
        var u = new RectangleF(ScreenR.X + lo.X * ScreenR.Width / lcdW, ScreenR.Y + lo.Y * ScreenR.Height / lcdH, lo.Width * ScreenR.Width / lcdW, lo.Height * ScreenR.Height / lcdH);
        hits.Add(new KeyValuePair<RectangleF, int>(ToWin(u), index));
    }
    Bitmap LcdCanvas(float t) {
        int Wp = Math.Max(200, (int)Math.Round(ScreenR.Width * devS)), Hp = Math.Max(112, (int)Math.Round(ScreenR.Height * devS));
        if (lcdBmp == null || lcdBmp.Width != Wp || lcdBmp.Height != Hp) { if (lcdBmp != null) lcdBmp.Dispose(); lcdBmp = new Bitmap(Wp, Hp, PixelFormat.Format32bppPArgb); }
        lcdW = 340; lcdH = (int)Math.Round(340f * Hp / Wp);
        int W = lcdW, H = lcdH;
        if (osdFontH != H) {
            osdFontH = H;
            osdBig = new Font("Segoe UI Semibold", H / 13.5f, GraphicsUnit.Pixel);
            osdRow = new Font("Segoe UI Semibold", H / 17.5f, GraphicsUnit.Pixel);
            osdSmall = new Font("Segoe UI", H / 21f, GraphicsUnit.Pixel);
            osdTiny = new Font("Segoe UI Semibold", H / 29f, GraphicsUnit.Pixel);
            osdLogo = new Font("Segoe UI Light", H / 5.2f, GraphicsUnit.Pixel);
        }
        DateTime now = DateTime.Now;
        bool rec = job != null;
        if (rec && !recOn) recSince = now;
        if (!rec && recOn) recTotal += now - recSince;
        recOn = rec;
        using (var o = Graphics.FromImage(lcdBmp)) {
            bool splash = starting && job == null && pages.Count == 0;
            // Cleared first: the scene's edge pixels are not fully opaque, so the last frame showed through and built up.
            o.Clear(Color.FromArgb(6, 8, 14));
            if (!splash) o.DrawImageUnscaled(Scene(Wp, Hp), 0, 0);
            o.ScaleTransform(Wp / (float)W, Hp / (float)H);
            o.SmoothingMode = SmoothingMode.AntiAlias;
            o.TextRenderingHint = System.Drawing.Text.TextRenderingHint.AntiAliasGridFit;
            float foot = osdSmall.Height + 4, lane = 15, top = osdRow.Height + 7, bottom = H - foot - lane - 2;
            if (splash) LcdSplash(o, W, H, t);
            else {
                LcdStatus(o, W, now);
                if (job != null) LcdJob(o, W, top, bottom, t);
                else if (pages.Count > 0) LcdPage(o, pages.Peek(), W, top, bottom, t);
            }
            LcdBoard(o, t, H - foot - 2, W);
            LcdFoot(o, W, H, now, splash);
        }
        return lcdBmp;
    }
    // What the camera is pointed at: a skate spot at dusk (stairs with a handrail, a ledge), dimmed under the menus.
    Bitmap Scene(int W, int H) {
        if (sceneBmp != null && sceneBmp.Width == W && sceneBmp.Height == H) return sceneBmp;
        if (sceneBmp != null) sceneBmp.Dispose();
        sceneBmp = new Bitmap(W, H, PixelFormat.Format32bppPArgb);
        using (var g = Graphics.FromImage(sceneBmp)) {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            float hz = H * 0.62f;
            using (var b = new LinearGradientBrush(new RectangleF(0, -1, W, hz + 2), Color.FromArgb(34, 46, 104), Color.FromArgb(222, 128, 78), 90f)) g.FillRectangle(b, 0, 0, W, hz);
            using (var b = new SolidBrush(Color.FromArgb(200, 252, 210, 136))) g.FillEllipse(b, W * 0.68f, hz - H * 0.17f, H * 0.16f, H * 0.16f);
            var rnd = new Random(5);
            using (var b = new SolidBrush(Color.FromArgb(44, 34, 56))) for (float x = 0; x < W; ) { float bw = W * (0.024f + (float)rnd.NextDouble() * 0.045f), bh = H * (0.05f + (float)rnd.NextDouble() * 0.18f); g.FillRectangle(b, x, hz - bh, bw, bh); x += bw + 1; }
            using (var b = new LinearGradientBrush(new RectangleF(0, hz - 1, W, H - hz + 2), Color.FromArgb(82, 68, 70), Color.FromArgb(30, 26, 30), 90f)) g.FillRectangle(b, 0, hz, W, H - hz);
            float sx0 = W * 0.6f, sy0 = H * 0.7f, stepW = W * 0.08f, stepH = H * 0.06f;
            var stairs = new List<PointF> { new PointF(sx0, H), new PointF(sx0, sy0) };
            for (int i = 0; i < 5; i++) { stairs.Add(new PointF(sx0 + (i + 1) * stepW, sy0 + i * stepH)); stairs.Add(new PointF(sx0 + (i + 1) * stepW, sy0 + (i + 1) * stepH)); }
            stairs.Add(new PointF(W, sy0 + 5 * stepH)); stairs.Add(new PointF(W, H));
            using (var b = new SolidBrush(Color.FromArgb(54, 48, 54))) g.FillPolygon(b, stairs.ToArray());
            float lw = Math.Max(1, H / 190f);
            using (var pen = new Pen(Color.FromArgb(130, 222, 156, 112), lw)) for (int i = 0; i < 5; i++) g.DrawLine(pen, sx0 + i * stepW, sy0 + i * stepH, sx0 + (i + 1) * stepW, sy0 + i * stepH);
            using (var b = new SolidBrush(Color.FromArgb(74, 68, 70))) g.FillRectangle(b, W * 0.05f, H * 0.74f, W * 0.3f, H * 0.07f);
            using (var pen = new Pen(Color.FromArgb(150, 222, 156, 112), lw)) g.DrawLine(pen, W * 0.05f, H * 0.74f, W * 0.35f, H * 0.74f);
            using (var pen = new Pen(Color.FromArgb(170, 176, 178, 190), lw * 1.4f)) { g.DrawLine(pen, sx0 + stepW * 0.5f, sy0 - H * 0.09f, W, sy0 - H * 0.09f + (W - sx0 - stepW * 0.5f) * stepH / stepW); for (int i = 0; i < 4; i++) { float px = sx0 + stepW * (0.5f + i * 1.2f); g.DrawLine(pen, px, sy0 - H * 0.09f + (px - sx0 - stepW * 0.5f) * stepH / stepW, px, sy0 + i * 1.2f * stepH); } }
            using (var b = new SolidBrush(Color.FromArgb(86, 0, 0, 0))) g.FillRectangle(b, 0, 0, W, H);
            using (var path = new GraphicsPath()) {
                path.AddEllipse(-W * 0.2f, -H * 0.25f, W * 1.4f, H * 1.5f);
                using (var outside = new Region(new RectangleF(0, 0, W, H))) { outside.Exclude(path); using (var b = new SolidBrush(Color.FromArgb(70, 0, 0, 0))) g.FillRegion(b, outside); }
            }
        }
        return sceneBmp;
    }
    static void ShadowText(Graphics g, string s, Font f, Color c, float x, float y) {
        using (var sh = new SolidBrush(Color.FromArgb(170, 0, 0, 0))) g.DrawString(s, f, sh, x + 0.6f, y + 0.7f);
        using (var b = new SolidBrush(c)) g.DrawString(s, f, b, x, y);
    }
    // A translucent glass panel with a fine light edge.
    static void Glass(Graphics g, RectangleF r, float rad) {
        using (var path = Round(r, rad)) {
            using (var b = new LinearGradientBrush(Grow(r, 1), Color.FromArgb(196, 44, 54, 80), Color.FromArgb(222, 10, 13, 22), 90f)) g.FillPath(b, path);
            using (var pen = new Pen(Color.FromArgb(76, 255, 255, 255), 0.6f)) g.DrawPath(pen, path);
        }
        using (var pen = new Pen(Color.FromArgb(46, 255, 255, 255), 0.6f)) g.DrawLine(pen, r.X + rad, r.Y + 1, r.Right - rad, r.Y + 1);
    }
    // A glossy bar: its gradient and a sheen over the top half.
    static void Gloss(Graphics g, RectangleF r, float rad, Color top, Color bottom) {
        using (var path = Round(r, rad)) using (var b = new LinearGradientBrush(Grow(r, 0.5f), top, bottom, 90f)) g.FillPath(b, path);
        var sheen = new RectangleF(r.X + 0.8f, r.Y + 0.5f, r.Width - 1.6f, r.Height * 0.48f);
        if (sheen.Width > 1 && sheen.Height > 0.5f)
            using (var path = Round(sheen, rad * 0.8f)) using (var b = new LinearGradientBrush(Grow(sheen, 0.5f), Color.FromArgb(120, 255, 255, 255), Color.FromArgb(18, 255, 255, 255), 90f)) g.FillPath(b, path);
    }
    void Pill(Graphics g, string s, Color c, float x, float y) {
        var size = g.MeasureString(s, osdTiny);
        var r = new RectangleF(x, y, size.Width + 3, osdTiny.Height + 2);
        using (var path = Round(r, r.Height / 2)) {
            using (var b = new SolidBrush(Fade(c, 46))) g.FillPath(b, path);
            using (var pen = new Pen(Fade(c, 200), 0.7f)) g.DrawPath(pen, path);
        }
        Write(g, s, osdTiny, c, r, StringAlignment.Center, StringAlignment.Center, false);
    }
    // Top row: STBY / REC with its glowing dot, the HDV badge, the tape counter (runs only while recording), the battery.
    void LcdStatus(Graphics o, int W, DateTime now) {
        TimeSpan tape = recTotal + (recOn ? now - recSince : TimeSpan.Zero);
        float x = 5, y = 3, fh = osdRow.Height;
        if (recOn) {
            float d = fh * 0.46f, dy = y + (fh - d) / 2;
            if (now.Millisecond < 650) {
                using (var b = new SolidBrush(Color.FromArgb(80, RecRed))) o.FillEllipse(b, x - d * 0.35f, dy - d * 0.35f, d * 1.7f, d * 1.7f);
                using (var b = new SolidBrush(RecRed)) o.FillEllipse(b, x, dy, d, d);
            }
            x += d + 3;
        }
        string mode = recOn ? "REC" : "STBY";
        ShadowText(o, mode, osdRow, recOn ? LcdRed : LcdInk, x, y); x += o.MeasureString(mode, osdRow).Width + 3;
        var hs = o.MeasureString("HDV 1080i", osdTiny);
        var hb = new RectangleF(x, y + (fh - osdTiny.Height - 1) / 2, hs.Width + 2, osdTiny.Height + 1);
        using (var path = Round(hb, 1.5f)) { using (var b = new SolidBrush(Color.FromArgb(70, 0, 0, 0))) o.FillPath(b, path); using (var pen = new Pen(Color.FromArgb(210, 255, 255, 255), 0.7f)) o.DrawPath(pen, path); }
        Write(o, "HDV 1080i", osdTiny, LcdInk, hb, StringAlignment.Center, StringAlignment.Center, false);
        x += hb.Width + 6;
        ShadowText(o, string.Format("{0}:{1:00}:{2:00}:{3:00}", (int)tape.TotalHours, tape.Minutes, tape.Seconds, tape.Milliseconds * 30 / 1000), osdRow, LcdInk, x, y);
        string mins = "120 min";
        var ms = o.MeasureString(mins, osdSmall);
        float bw = fh * 1.2f, bh = fh * 0.52f, bx = W - 5 - ms.Width - bw - 3, by = y + (fh - bh) / 2;
        var body = new RectangleF(bx, by, bw, bh);
        using (var path = Round(body, 1.5f)) { using (var b = new SolidBrush(Color.FromArgb(90, 0, 0, 0))) o.FillPath(b, path); using (var pen = new Pen(Color.FromArgb(220, 255, 255, 255), 0.8f)) o.DrawPath(pen, path); }
        using (var b = new SolidBrush(Color.FromArgb(220, 255, 255, 255))) o.FillRectangle(b, bx + bw, by + bh * 0.3f, 1.4f, bh * 0.4f);
        float seg = (bw - 2.4f) / 4;
        for (int i = 0; i < 4; i++) Gloss(o, new RectangleF(bx + 1.2f + i * seg + 0.3f, by + 1.2f, seg - 0.6f, bh - 2.4f), 0.6f, Color.FromArgb(170, 240, 110), Color.FromArgb(66, 168, 40));
        ShadowText(o, mins, osdSmall, LcdInk, W - 5 - ms.Width, y + (fh - osdSmall.Height) / 2);
    }
    // A controller key badge and its label, for the hint row.
    void Badge(Graphics o, ref float x, float y, string key, Color c, string label) {
        float d = osdSmall.Height * 0.9f;
        var r = key.Length == 1 ? new RectangleF(x, y + (osdSmall.Height - d) / 2, d, d) : new RectangleF(x, y + (osdSmall.Height - d) / 2, o.MeasureString(key, osdTiny).Width + 3, d);
        Gloss(o, r, d / 2, Mix(c, Color.White, 0.25f), Mix(c, Color.Black, 0.2f));
        Write(o, key, osdTiny, Color.White, r, StringAlignment.Center, StringAlignment.Center, false);
        x = r.Right + 2;
        ShadowText(o, label, osdSmall, LcdInk, x, y);
        x += o.MeasureString(label, osdSmall).Width + 5;
    }
    static Color Mix(Color a, Color b, float k) { return Color.FromArgb(a.A, (int)(a.R + (b.R - a.R) * k), (int)(a.G + (b.G - a.G) * k), (int)(a.B + (b.B - a.B) * k)); }
    // Bottom: what the keys do now on the left, the date and time stamp on the right.
    void LcdFoot(Graphics o, int W, int H, DateTime now, bool splash) {
        float y = H - osdSmall.Height - 3, x = 5;
        Color a = Color.FromArgb(92, 186, 72), b = Color.FromArgb(220, 70, 60);
        bool cancel = job != null && ((build.Waiting != null && !build.Ended) || build.FetchWaiting());
        if (closeAt >= 0) ShadowText(o, "Closing in " + closeAt + " s", osdSmall, LcdInk, x, y);
        else if (cancel) Badge(o, ref x, y, "B", b, cancelArmed ? "Again to cancel" : "Cancel");
        else if (splash) { if (CanSkip()) Badge(o, ref x, y, "B", b, skipArmed ? "Again to skip" : "Skip the check"); }
        else if (job == null) {
            Badge(o, ref x, y, "A", a, "Select");
            if (pages.Count > 0 && IsMain(pages.Peek())) Badge(o, ref x, y, "START/STOP", RecRed, "Play");
            else Badge(o, ref x, y, "B", b, "Back");
        }
        string stamp = now.ToString("MMM d yyyy   h:mm tt", System.Globalization.CultureInfo.InvariantCulture).ToUpperInvariant();
        ShadowText(o, stamp, osdSmall, LcdInk, W - 5 - o.MeasureString(stamp, osdSmall).Width, y);
    }
    // The board rides a lane along the bottom of the screen and pops an ollie now and then (USER-ASKS 488).
    void LcdBoard(Graphics o, float t, float ground, int W) {
        const float len = 44;
        float x = -len + (t * vary[17] * 0.25f + vary[18]) % (W + len);
        float cycle = t % vary[19], jump = 0, tilt = 0;
        if (cycle < 0.8f) { float q = cycle / 0.8f; jump = (float)Math.Sin(Math.PI * q) * 15; tilt = q < 0.25f ? -18 * q / 0.25f : -18 * (1 - (q - 0.25f) / 0.75f); }
        using (var pen = new Pen(Color.FromArgb(46, 255, 255, 255), 0.6f)) o.DrawLine(pen, 5, ground + 0.5f, W - 5, ground + 0.5f);
        using (var b = new SolidBrush(Color.FromArgb(Math.Max(0, (int)(110 * (1 - jump / 24))), 0, 0, 0))) o.FillEllipse(b, x + len * 0.1f + jump * 0.3f, ground - 1.2f, len * 0.8f - jump * 0.6f, 2.6f);
        float y = ground - len * 34f / 180f - jump;
        var st = o.Save();
        float px = x + len / 2, py = y + len * 17f / 180f;
        o.TranslateTransform(px, py); o.RotateTransform(tilt); o.TranslateTransform(-px, -py);
        Board(o, x, y, len);
        o.Restore(st);
    }
    // A menu screen: the title tab, the list with the amber selection bar, the selected row's info beside it; a text
    // page shows its text in a glass box (and a build's first error under it).
    void LcdPage(Graphics o, Page p, int W, float top, float bottom, float t) {
        const float m = 5;
        string title = IsMain(p) ? "HOME" : p.Title.TrimEnd(':', ' ');
        var tab = new RectangleF(m, top, Math.Min(W - 2 * m, o.MeasureString(title, osdRow).Width + 10), osdRow.Height + 3);
        Gloss(o, tab, tab.Height / 2, Color.FromArgb(240, 88, 152, 242), Color.FromArgb(240, 26, 74, 168));
        Write(o, title, osdRow, Color.White, tab, StringAlignment.Center, StringAlignment.Center, false);
        float y = tab.Bottom + 3;
        if (p.Body != null) {
            var br = new RectangleF(m, y, W - 2 * m, p.Items.Count > 0 ? (bottom - y) * 0.6f : bottom - y);
            Glass(o, br, 5);
            float textH = br.Height - 6;
            if (p.Mono != null) textH = Math.Min(textH, o.MeasureString(p.Body, osdSmall, (int)(br.Width - 10)).Height + 2);
            Write(o, p.Body, osdSmall, LcdInk, new RectangleF(br.X + 5, br.Y + 3, br.Width - 10, textH), StringAlignment.Near, StringAlignment.Near, true);
            if (p.Mono != null) {
                var eb = new RectangleF(br.X + 4, br.Y + 4 + textH, br.Width - 8, br.Bottom - 4 - (br.Y + 4 + textH));
                if (eb.Height > osdSmall.Height) {
                    using (var path = Round(eb, 3)) { using (var b = new SolidBrush(Color.FromArgb(190, 70, 14, 18))) o.FillPath(b, path); using (var pen = new Pen(Fade(LcdRed, 200), 0.7f)) o.DrawPath(pen, path); }
                    Write(o, p.Mono, osdSmall, LcdInk, new RectangleF(eb.X + 4, eb.Y + 2, eb.Width - 8, eb.Height - 4), StringAlignment.Near, StringAlignment.Near, true);
                }
            }
            y = br.Bottom + 3;
        }
        if (p.Items.Count == 0) return;
        bool split = p.Body == null;
        float listW = split ? (float)Math.Floor(W * 0.42f) : W - 2 * m, rowH = osdRow.Height + 4;
        int fit = Math.Max(1, (int)((bottom - y - 5) / rowH));
        int first = Math.Max(0, Math.Min(p.Sel - fit / 2, p.Items.Count - fit)), shown = Math.Min(fit, p.Items.Count - first);
        var box = new RectangleF(m, y, listW, shown * rowH + 5);
        Glass(o, box, 5);
        for (int i = first; i < first + shown; i++) {
            var it = p.Items[i]; bool sel = i == p.Sel;
            var r = new RectangleF(box.X + 2.5f, box.Y + 2.5f + (i - first) * rowH, box.Width - 5, rowH);
            if (sel) Gloss(o, r, 3, LcdAmberHi, LcdAmberLo);
            string text = split ? ShortName(it.Text) : it.Text;
            bool cur = text.StartsWith("> ");
            if (cur) text = text.Substring(2);
            Write(o, text, osdRow, sel ? Color.FromArgb(36, 22, 6) : it.On ? LcdInk : Color.FromArgb(120, 128, 144), new RectangleF(r.X + 5, r.Y, r.Width - (cur ? 14 : 7), r.Height), StringAlignment.Near, StringAlignment.Center, false);
            if (cur) using (var b = new SolidBrush(sel ? Color.FromArgb(36, 22, 6) : LcdAmber)) o.FillEllipse(b, r.Right - 8, r.Y + rowH / 2 - 2, 4, 4);
            LoHit(r, i);
        }
        using (var pen = new Pen(Color.FromArgb(200, 255, 255, 255), 1f)) {
            float ax = box.Right - 9;
            if (first > 0) o.DrawLines(pen, new[] { new PointF(ax, box.Y + 3), new PointF(ax + 3, box.Y + 0.5f), new PointF(ax + 6, box.Y + 3) });
            if (first + shown < p.Items.Count) o.DrawLines(pen, new[] { new PointF(ax, box.Bottom - 3), new PointF(ax + 3, box.Bottom - 0.5f), new PointF(ax + 6, box.Bottom - 3) });
        }
        if (split) LcdInfo(o, p, new RectangleF(box.Right + 4, y, W - m - box.Right - 4, bottom - y), t);
    }
    void LcdInfo(Graphics o, Page p, RectangleF r, float t) {
        Glass(o, r, 5);
        float x = r.X + 6, w = r.Width - 12, y = r.Y + 4, bottom = r.Bottom - 4, sh = osdSmall.Height;
        var it = p.Items[p.Sel];
        if (IsMain(p) && p.Sel == 0) {
            Write(o, "NOW SELECTED", osdTiny, LcdAmber, new RectangleF(x, y, w, osdTiny.Height + 2), StringAlignment.Near, StringAlignment.Near, false); y += osdTiny.Height + 1;
            float vh = Math.Min(o.MeasureString(St("version"), osdBig, (int)w).Height, osdBig.Height * 2.2f);
            Write(o, St("version"), osdBig, LcdInk, new RectangleF(x, y, w, vh), StringAlignment.Near, StringAlignment.Near, true); y += vh + 1;
            Write(o, St("version_status"), osdSmall, LcdInk, new RectangleF(x, y, w, sh * 2.2f), StringAlignment.Near, StringAlignment.Near, true); y += sh * 2.2f;
            string send = St("send");
            Write(o, "Last session: " + St("last") + (send == "" || send == "off" ? "" : "   Send: " + send), osdSmall, LcdDim, new RectangleF(x, y, w, bottom - y - osdRow.Height - 6), StringAlignment.Near, StringAlignment.Near, true);
            float pulse = 0.55f + 0.45f * (float)Math.Sin(t * 3.2);
            var play = new RectangleF(x, bottom - osdRow.Height - 4, w, osdRow.Height + 4);
            Gloss(o, play, play.Height / 2, Fade(Color.FromArgb(242, 74, 62), (int)(150 + 90 * pulse)), Fade(Color.FromArgb(160, 24, 18), (int)(150 + 90 * pulse)));
            Write(o, "Press START/STOP to play", osdRow, Color.White, play, StringAlignment.Center, StringAlignment.Center, false);
            return;
        }
        string name = it.Text.StartsWith("> ") ? it.Text.Substring(2) : it.Text;
        float nh = Math.Min(o.MeasureString(name, osdRow, (int)w).Height, osdRow.Height * 2.2f);
        Write(o, name, osdRow, LcdInk, new RectangleF(x, y, w, nh), StringAlignment.Near, StringAlignment.Near, true); y += nh + 2;
        Pill(o, it.On ? "READY" : "NOT AVAILABLE", it.On ? LcdGreen : LcdRed, x, y); y += osdTiny.Height + 5;
        string info = it.Info != "" ? it.Info : it.Hint;
        if (info != "") {
            float ih = Math.Min(o.MeasureString(info, osdSmall, (int)w).Height, bottom - y);
            Write(o, info, osdSmall, LcdInk, new RectangleF(x, y, w, ih), StringAlignment.Near, StringAlignment.Near, true); y += ih + 2;
        }
        float nameW = w * 0.38f;
        using (var line = new Pen(Color.FromArgb(40, 255, 255, 255), 0.5f))
        foreach (var rec in it.Records) {
            if (y + sh > bottom) { Write(o, "...", osdSmall, LcdDim, new RectangleF(x, y - 2, w, sh + 2), StringAlignment.Near, StringAlignment.Near, false); break; }
            int cut = rec.IndexOf(": ");
            string a = cut > 0 ? rec.Substring(0, cut) : rec, b = cut > 0 ? rec.Substring(cut + 2) : "";
            if (a.StartsWith("#")) { Write(o, a.Substring(1).ToUpperInvariant() + (b != "" ? "  " + b : ""), osdTiny, LcdAmber, new RectangleF(x, y + 1, w, osdTiny.Height + 2), StringAlignment.Near, StringAlignment.Near, false); y += osdTiny.Height + 3; continue; }
            float rh = Math.Min(Math.Max(o.MeasureString(a, osdSmall, (int)nameW).Height, o.MeasureString(b, osdSmall, (int)(w - nameW - 3)).Height), Math.Max(sh, bottom - y));
            o.DrawLine(line, x, y, x + w, y);
            Write(o, a, osdSmall, LcdInk, new RectangleF(x, y, nameW, rh), StringAlignment.Near, StringAlignment.Near, true);
            Write(o, b, osdSmall, LcdDim, new RectangleF(x + nameW + 3, y, w - nameW - 3, rh), StringAlignment.Near, StringAlignment.Near, true);
            y += rh + 1;
        }
    }
    // A job (fetch, build): its name, the time, a glossy progress bar for cargo's step count, the counts, then the first
    // error or the newest log lines.
    void LcdJob(Graphics o, int W, float top, float bottom, float t) {
        const float m = 5;
        var card = new RectangleF(m, top, W - 2 * m, bottom - top);
        Glass(o, card, 6);
        float x = card.X + 6, w = card.Width - 12, y = card.Y + 4, sh = osdSmall.Height;
        bool isBuild = build.Active || build.Ended, failed = build.Ended && !build.Ok, fetchWait = build.FetchWaiting();
        float k = isBuild && build.Total > 0 ? Math.Min(1f, build.Done / (float)build.Total) : -1;
        var el = DateTime.Now - (isBuild ? build.Start : fetchWait ? build.PreWaitSince : jobStart);
        Write(o, isBuild ? build.Name : jobTitle, osdBig, LcdInk, new RectangleF(x, y, w * 0.78f, osdBig.Height + 2), StringAlignment.Near, StringAlignment.Near, false);
        Write(o, BuildView.Clock(el), osdRow, LcdDim, new RectangleF(x, y + 1, w, osdBig.Height + 2), StringAlignment.Far, StringAlignment.Near, false);
        y += osdBig.Height + 1;
        if (isBuild) { Write(o, "at " + build.Commit, osdSmall, LcdDim, new RectangleF(x, y, w, sh + 2), StringAlignment.Near, StringAlignment.Near, false); y += sh + 2; }
        var bar = new RectangleF(x, y, w, 6);
        using (var path = Round(bar, 3)) { using (var b = new SolidBrush(Color.FromArgb(150, 0, 0, 0))) o.FillPath(b, path); using (var pen = new Pen(Color.FromArgb(60, 255, 255, 255), 0.6f)) o.DrawPath(pen, path); }
        Color hi = failed ? Color.FromArgb(255, 130, 120) : LcdAmberHi, lo = failed ? Color.FromArgb(200, 40, 30) : LcdAmberLo;
        if (k >= 0) { if (k > 0) Gloss(o, new RectangleF(bar.X, bar.Y, Math.Max(bar.Height, bar.Width * k), bar.Height), 3, hi, lo); }
        else {
            float run = (t * 0.45f) % 1f, segX = bar.X + run * bar.Width * 1.3f - bar.Width * 0.3f;
            var seg = RectangleF.Intersect(bar, new RectangleF(segX, bar.Y, bar.Width * 0.3f, bar.Height));
            if (seg.Width > 1) Gloss(o, seg, 3, hi, lo);
        }
        y += 10;
        if (isBuild) {
            string count = build.Total > 0 ? build.Done + " / " + build.Total + " build steps" : build.Compiled + " crates compiled";
            if (!build.Ended && build.Waiting != null) count = "Waiting " + BuildView.Clock(DateTime.Now - build.WaitSince) + ": " + build.Waiting;
            else if (!build.Ended && build.Total == 0 && build.Compiled == 0 && build.StageText != null) count = build.StageText;
            if (build.Ended) count = build.Ok ? "Built in " + BuildView.Clock(build.End - build.Start) : "Build failed";
            Write(o, count, osdRow, failed ? LcdRed : LcdInk, new RectangleF(x, y, w * 0.7f, osdRow.Height + 2), StringAlignment.Near, StringAlignment.Near, false);
            if (build.Warnings > 0) { string wt = build.Warnings + (build.Warnings == 1 ? " warning" : " warnings"); Pill(o, wt, LcdAmber, x + w - o.MeasureString(wt, osdTiny).Width - 3, y + 1); }
            y += osdRow.Height + 2;
            string line = !build.Ended && build.Waiting != null ? (build.WaitWho ?? "Cargo does not say who; it continues by itself once that build ends.") : (build.Now != "" && !build.Ended ? "Now: " + build.Now : "");
            if (line != "") { Write(o, line, osdSmall, LcdDim, new RectangleF(x, y, w, sh * 2.2f), StringAlignment.Near, StringAlignment.Near, true); y += sh * 2.2f; }
            if (build.Error.Count > 0) {
                var eb = new RectangleF(x, y, w, card.Bottom - 4 - y);
                using (var path = Round(eb, 3)) { using (var b = new SolidBrush(Color.FromArgb(190, 70, 14, 18))) o.FillPath(b, path); using (var pen = new Pen(Fade(LcdRed, 200), 0.7f)) o.DrawPath(pen, path); }
                Write(o, "First error\n" + string.Join("\n", build.Error), osdSmall, LcdInk, new RectangleF(eb.X + 4, eb.Y + 2, eb.Width - 8, eb.Height - 4), StringAlignment.Near, StringAlignment.Near, true);
            }
            return;
        }
        var lines = new List<string>();
        for (int i = log.Count - 1; i >= 0 && lines.Count < 12; i--) { string c = Clean(log[i]); if (c != "") lines.Add(c); }
        string status = fetchWait ? "Waiting " + BuildView.Clock(el) + ": " + build.PreWait : lines.Count > 0 ? lines[0] : "Starting...";
        Write(o, status, osdRow, LcdInk, new RectangleF(x, y, w, osdRow.Height + 2), StringAlignment.Near, StringAlignment.Near, false); y += osdRow.Height + 2;
        if (fetchWait) { Write(o, "git stops by itself if nothing arrives for 30 s; cancelling keeps the last fetched version.", osdSmall, LcdDim, new RectangleF(x, y, w, sh * 2.2f), StringAlignment.Near, StringAlignment.Near, true); y += sh * 2.2f; }
        for (int i = 1; i < lines.Count && y + sh <= card.Bottom - 3; i++) { Write(o, lines[i], osdSmall, Fade(LcdDim, Math.Max(90, 255 - i * 22)), new RectangleF(x, y, w, sh + 1), StringAlignment.Near, StringAlignment.Near, false); y += sh; }
    }
    // Power-on screen: the logo, the model, then the start check's step with three pulsing dots.
    void LcdSplash(Graphics o, int W, int H, float t) {
        float since = (float)(DateTime.Now - startSince).TotalSeconds;
        var ls = o.MeasureString("SKATE", osdLogo);
        float lx = (W - ls.Width) / 2, ly = H * 0.16f;
        Write(o, "SKATE", osdLogo, LcdInk, new RectangleF(lx, ly, ls.Width + 4, ls.Height), StringAlignment.Near, StringAlignment.Near, false);
        Gloss(o, new RectangleF(lx + ls.Width - 2, ly + ls.Height * 0.22f, osdRow.Height * 1.4f, osdRow.Height), 3, Color.FromArgb(88, 152, 242), Color.FromArgb(26, 74, 168));
        Write(o, "HD", osdRow, Color.White, new RectangleF(lx + ls.Width - 2, ly + ls.Height * 0.22f, osdRow.Height * 1.4f, osdRow.Height), StringAlignment.Center, StringAlignment.Center, false);
        Write(o, "SK8-1000  HIGH DEFINITION", osdTiny, LcdDim, new RectangleF(0, ly + ls.Height * 0.95f, W, osdTiny.Height + 2), StringAlignment.Center, StringAlignment.Near, false);
        string stage = skipped ? "Finishing the start" : (startStage ?? "Checking for a newer launcher");
        Write(o, stage, osdRow, LcdInk, new RectangleF(0, H * 0.6f, W, osdRow.Height + 2), StringAlignment.Center, StringAlignment.Near, false);
        for (int i = 0; i < 3; i++) {
            float a = 0.35f + 0.65f * Math.Max(0, (float)Math.Sin(t * 4 - i * 0.7));
            using (var b = new SolidBrush(Fade(LcdAmber, (int)(255 * a)))) o.FillEllipse(b, W / 2f - 8 + i * 7, H * 0.6f + osdRow.Height + 5, 3.4f, 3.4f);
        }
        if (since >= 3) ShadowText(o, BuildView.Clock(DateTime.Now - startSince), osdSmall, LcdDim, 5, 4);
    }

    // A play option's button label: the panel shows the full name, so the button drops the "Play + " / "Play with the "
    // lead ("Play + trace all" -> "Trace all").
    // Other long names get a short button label too, and a trailing " (...)" note (version names) goes to the panel only.
    static readonly Dictionary<string, string> ShortLabels = new Dictionary<string, string> {
        { "Update and build the selected version", "Update and build" },
        { "Clear builds and build caches", "Clear builds" },
        { "Send the logs now", "Send logs" },
        { "Recomp: get the newest build", "Recomp: newest build" },
        { "Recomp: get the main PC save", "Recomp: main PC save" },
    };
    static string ShortName(string name) {
        string current = name.StartsWith("> ") ? "> " : "";
        name = name.Substring(current.Length);
        string label;
        if (ShortLabels.TryGetValue(name, out label)) return current + label;
        foreach (var lead in new[] { "Play + ", "Play with the ", "Play with " })
            if (name.StartsWith(lead) && name.Length > lead.Length) return current + char.ToUpper(name[lead.Length]) + name.Substring(lead.Length + 1);
        int note = name.IndexOf(" (");
        if (note > 0 && name.EndsWith(")")) name = name.Substring(0, note);
        return current + name;
    }

}

static class SkateLauncher {
    // One launcher per folder (laptop 2026-10-19: two windows were open by accident; one held its build log open and
    // the after-session send failed). The self-update restart releases it before starting the new window.
    public static System.Threading.Mutex Instance;
    public static void ReleaseInstance() {
        if (Instance == null) return;
        try { Instance.ReleaseMutex(); } catch { }
        Instance.Dispose(); Instance = null;
    }
    [STAThread]
    static void Main(string[] argv) {
        Application.EnableVisualStyles();
        // SkateLauncher.exe --snapshot <out.png> [main|versions|tools|job|build|failed|waiting|fetching|checking] [<width>x<height>]: draws one screen (default
        // 1920x1080; the window's own size to check the windowed layout) and exits (checking the look without the window).
        // SkateLauncher.exe --bench <out.txt> [<width>x<height>]: times Render across the power-on and settled frames
        // (ms per frame, plus the hand-over to the layered window) into out.txt.
        // SkateLauncher.exe --wear-test <out.txt> <out.png>: one start's wear roll (as a real start rolls it), checked:
        // every pixel the marks change lies on the camera's parts (never the LCD glass or off the camera), and no tag or
        // doodle spills over its part. Appends PASS / FAIL with the counts to out.txt, saves the frame to out.png.
        if (argv.Length >= 3 && argv[0] == "--wear-test") {
            var form = new LauncherForm(new string[0]);
            File.AppendAllText(argv[1], form.WearTest(argv[2]) + Environment.NewLine);
            return;
        }
        if (argv.Length >= 2 && argv[0] == "--bench") {
            var form = new LauncherForm(new string[0]);
            int bw = 1243, bh = 777;
            if (argv.Length > 2) { var wh = argv[2].Split('x'); bw = int.Parse(wh[0]); bh = int.Parse(wh[1]); }
            File.WriteAllText(argv[1], form.Bench(bw, bh));
            return;
        }
        if (argv.Length >= 2 && argv[0] == "--snapshot") {
            var form = new LauncherForm(new string[0]);
            int sw = 1920, sh = 1080;
            if (argv.Length > 3) { var wh = argv[3].Split('x'); sw = int.Parse(wh[0]); sh = int.Parse(wh[1]); }
            form.Snapshot(argv[1], argv.Length > 2 ? argv[2] : "main", sw, sh);
            return;
        }
        string folder = Path.GetFullPath(AppDomain.CurrentDomain.BaseDirectory).TrimEnd('\\').ToLowerInvariant().Replace('\\', '/');
        bool first;
        Instance = new System.Threading.Mutex(true, "Local\\SkateLauncher-" + folder, out first);
        if (!first) {
            // Already running: bring its window to the front instead of opening a second one.
            int me = Process.GetCurrentProcess().Id;
            foreach (var name in new[] { "SkateLauncher", "SkateLauncher.old" })
                foreach (var other in Process.GetProcessesByName(name))
                    if (other.Id != me && other.MainWindowHandle != IntPtr.Zero && Native.IsWindowVisible(other.MainWindowHandle)) { Native.Front(other.MainWindowHandle); return; }
            return;
        }
        Application.Run(new LauncherForm(argv));
        ReleaseInstance();
    }
}
