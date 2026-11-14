# Capture the dev recomp's window (only it, even when behind other windows) every $Every seconds
# into $Out (shot_<unix ms>.jpg; the trace's CLOCK line gives its unix-ms time zero) until $Stop exists. Uses PrintWindow PW_RENDERFULLCONTENT,
# like the recomp's own F6 screenshot; also drops the recomp to below-normal priority once found
# (unless -KeepPriority).
# The dev build is recognized by its exe folder: -ExeDir or $env:RECOMP_EXE_DIR (set by recomp_script_run.sh).
param([string]$Out, [string]$Stop, [double]$Every = 2, [switch]$KeepPriority, [string]$ExeDir = $env:RECOMP_EXE_DIR)
if (-not $ExeDir) { Write-Host 'set RECOMP_EXE_DIR or pass -ExeDir'; exit 2 }
$prefix = $ExeDir.TrimEnd('\') + '\*'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Ri, B; }
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref System.Drawing.Point p);
}
"@ -ReferencedAssemblies System.Drawing
New-Item -ItemType Directory -Force $Out | Out-Null
$start = Get-Date
$lowered = [bool]$KeepPriority  # -KeepPriority: someone is playing it, leave the game at normal priority
while (-not (Test-Path $Stop)) {
  $p = Get-Process skate3 -ErrorAction SilentlyContinue |
       Where-Object { $_.Path -like $prefix } | Select-Object -First 1
  if ($p -and -not $lowered) { try { $p.PriorityClass = 'BelowNormal'; $lowered = $true } catch {} }
  if ($p -and $p.MainWindowHandle -ne [IntPtr]::Zero) {
    $h = $p.MainWindowHandle
    $wr = New-Object W+R; [void][W]::GetWindowRect($h, [ref]$wr)
    $w = $wr.Ri - $wr.L; $ht = $wr.B - $wr.T
    if ($w -gt 0 -and $ht -gt 0) {
      $bmp = New-Object System.Drawing.Bitmap $w, $ht
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      $dc = $g.GetHdc(); [void][W]::PrintWindow($h, $dc, 2); $g.ReleaseHdc($dc)
      $cr = New-Object W+R; [void][W]::GetClientRect($h, [ref]$cr)
      $pt = New-Object System.Drawing.Point 0, 0; [void][W]::ClientToScreen($h, [ref]$pt)
      $rect = New-Object System.Drawing.Rectangle ($pt.X - $wr.L), ($pt.Y - $wr.T), $cr.Ri, $cr.B
      if ($rect.Width -gt 0 -and $rect.Height -gt 0 -and $rect.Right -le $w -and $rect.Bottom -le $ht) {
        $full = $bmp.Clone($rect, $bmp.PixelFormat)
        # Half size, JPEG: ~60 KB per shot instead of ~4 MB (enough to see what happened).
        $crop = New-Object System.Drawing.Bitmap $full, ([int]($rect.Width / 2)), ([int]($rect.Height / 2))
        $full.Dispose()
        $ms = [DateTimeOffset]::Now.ToUnixTimeMilliseconds()
        $crop.Save((Join-Path $Out ("shot_{0}.jpg" -f $ms)), [System.Drawing.Imaging.ImageFormat]::Jpeg)
        $crop.Dispose()
      }
      $g.Dispose(); $bmp.Dispose()
    }
  }
  Start-Sleep -Milliseconds ([int]($Every * 1000))
}
