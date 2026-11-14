# Drop files from the Windows file cache without admin: opening a file unbuffered (FILE_FLAG_NO_BUFFERING) makes
# NTFS flush and purge its cached pages. For cold-start benches (USER-ASKS 471).
# usage: evict.ps1 <file or folder>...   (folders recursively; any path segment named in $env:EVICT_SKIP, comma list, is skipped)
param([Parameter(ValueFromRemainingArguments)][string[]]$Paths)
Add-Type -TypeDefinition @'
using System; using System.IO; using System.Runtime.InteropServices;
public static class Evict {
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
  static extern IntPtr CreateFileW(string n, uint a, uint s, IntPtr sa, uint c, uint f, IntPtr t);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  public static bool One(string path) {
    IntPtr h = CreateFileW(path, 0x80000000u, 7u, IntPtr.Zero, 3u, 0x20000000u, IntPtr.Zero);
    if (h == new IntPtr(-1)) return false;
    CloseHandle(h); return true;
  }
  public static int[] Tree(string root, string[] skip) {
    int n = 0, failed = 0;
    if (File.Exists(root)) { if (One(root)) n++; else failed++; return new[] { n, failed }; }
    var stack = new System.Collections.Generic.Stack<string>(); stack.Push(root);
    while (stack.Count > 0) {
      string dir = stack.Pop();
      foreach (var d in Directory.EnumerateDirectories(dir)) if (Array.IndexOf(skip, Path.GetFileName(d)) < 0) stack.Push(d);
      foreach (var f in Directory.EnumerateFiles(dir)) { if (One(f)) n++; else failed++; }
    }
    return new[] { n, failed };
  }
}
'@
$skip = if ($env:EVICT_SKIP) { $env:EVICT_SKIP.Split(',') } else { @() }
$sw = [Diagnostics.Stopwatch]::StartNew(); $n = 0; $failed = 0
foreach ($p in $Paths) { $r = [Evict]::Tree($p, [string[]]$skip); $n += $r[0]; $failed += $r[1] }
"evicted $n files ($failed failed) in $([int]$sw.Elapsed.TotalSeconds) s"
