# Launches the spike windowed and drives the title bar with real (SendInput-level) mouse and key input.
param([Parameter(Mandatory = $true)][string]$Exe, [Parameter(Mandatory = $true)][string]$Shot)
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern IntPtr MonitorFromPoint(POINT p, uint f);
  [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr m, int t, out uint x, out uint y);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; public override string ToString() { return Left+","+Top+" "+(Right-Left)+"x"+(Bottom-Top); } }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
'@
[void][W]::SetProcessDpiAwarenessContext([IntPtr]-4)  # per-monitor v2: physical pixels
function Rect($h) { $r = New-Object W+RECT; [void][W]::GetWindowRect($h, [ref]$r); $r }
function Click($x, $y) { [void][W]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 60; [W]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 50; [W]::mouse_event(4, 0, 0, 0, 0) }
function DoubleClick($x, $y) { Click $x $y; Start-Sleep -Milliseconds 90; Click $x $y }
function Drag($x, $y, $dx, $dy) {
  [void][W]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 80; [W]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 120
  for ($i = 1; $i -le 20; $i++) { [void][W]::SetCursorPos($x + $dx * $i / 20, $y + $dy * $i / 20); Start-Sleep -Milliseconds 20 }
  Start-Sleep -Milliseconds 300; [W]::mouse_event(4, 0, 0, 0, 0)
}
function Chord($keys) { foreach ($k in $keys) { [W]::keybd_event($k, 0, 0, 0) }; Start-Sleep -Milliseconds 60; [array]::Reverse($keys); foreach ($k in $keys) { [W]::keybd_event($k, 0, 2, 0) } }
function Report($name, $ok, $detail) { "{0,-34} {1,-4} {2}" -f $name, ($(if ($ok) { "PASS" } else { "FAIL" })), $detail }

foreach ($x in -1000, 1000) { $p = New-Object W+POINT; $p.X = $x; $p.Y = 500; $m = [W]::MonitorFromPoint($p, 2); $dx = 0; $dy = 0; [void][W]::GetDpiForMonitor($m, 0, [ref]$dx, [ref]$dy); "monitor at x=$x dpi $dx" }

$p = Start-Process -FilePath $Exe -PassThru
$deadline = (Get-Date).AddSeconds(30)
while ($p.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200; $p.Refresh() }
$h = $p.MainWindowHandle
Start-Sleep -Seconds 3
[void][W]::ShowWindow($h, 9); [void][W]::SetWindowPos($h, [IntPtr]::Zero, 100, 60, 1600, 900, 0x14); Start-Sleep -Milliseconds 800
[void][W]::SetForegroundWindow($h); Start-Sleep -Milliseconds 300
$dpi = [W]::GetDpiForWindow($h); $s = $dpi / 96.0
"window dpi $dpi, rect $(Rect $h)"

# Screenshot first, at rest.
$r = Rect $h; $bmp = New-Object System.Drawing.Bitmap ($r.Right - $r.Left), ($r.Bottom - $r.Top)
$g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc(); [void][W]::PrintWindow($h, $dc, 2); $g.ReleaseHdc($dc); $bmp.Save($Shot); $g.Dispose(); $bmp.Dispose()

function TitlePoint { $r = Rect $h; @(($r.Left + [int](150 * $s)), ($r.Top + [int](24 * $s))) }
function Caption($n) { $r = Rect $h; @(($r.Right - [int]((46 * $n - 23) * $s)), ($r.Top + [int](16 * $s))) }  # n: 1 close, 2 max, 3 min

$a = Rect $h; $t = TitlePoint; Drag $t[0] $t[1] 200 100; Start-Sleep -Milliseconds 500; $b = Rect $h
Report "drag title bar +200,+100" (($b.Left - $a.Left) -eq 200 -and ($b.Top - $a.Top) -eq 100) "$a -> $b"

$t = TitlePoint; DoubleClick $t[0] $t[1]; Start-Sleep -Milliseconds 800
Report "double-click title maximizes" ([W]::IsZoomed($h)) "rect $(Rect $h)"
$t = TitlePoint; DoubleClick $t[0] $t[1]; Start-Sleep -Milliseconds 800
Report "double-click again restores" (-not [W]::IsZoomed($h)) "rect $(Rect $h)"

$m = Caption 2; Click $m[0] $m[1]; Start-Sleep -Milliseconds 800
Report "maximize button" ([W]::IsZoomed($h)) "rect $(Rect $h)"
$m = Caption 2; Click $m[0] $m[1]; Start-Sleep -Milliseconds 800
Report "restore button" (-not [W]::IsZoomed($h)) "rect $(Rect $h)"

Chord @(0x5B, 0x25); Start-Sleep -Milliseconds 900; $r = Rect $h
Report "Win+Left snaps to left half" ($r.Left -le 0 -and $r.Left -ge -8 -and ($r.Right - $r.Left) -lt 1100) "rect $r"
[void][W]::ShowWindow($h, 9); [void][W]::SetWindowPos($h, [IntPtr]::Zero, 300, 200, 1200, 700, 0x14); Start-Sleep -Milliseconds 600

$t = TitlePoint; Drag $t[0] $t[1] 0 (2 - $t[1]); Start-Sleep -Milliseconds 900
Report "drag to top edge maximizes (snap)" ([W]::IsZoomed($h)) "rect $(Rect $h)"
[void][W]::ShowWindow($h, 9); [void][W]::SetWindowPos($h, [IntPtr]::Zero, 300, 200, 1200, 700, 0x14); Start-Sleep -Milliseconds 600

$d0 = [W]::GetDpiForWindow($h); [void][W]::SetWindowPos($h, [IntPtr]::Zero, -1700, 200, 1200, 700, 0x14); Start-Sleep -Milliseconds 900
$d1 = [W]::GetDpiForWindow($h); "moved to the left monitor: dpi $d0 -> $d1, rect $(Rect $h), process alive $(-not $p.HasExited)"
[void][W]::SetWindowPos($h, [IntPtr]::Zero, 300, 200, 1200, 700, 0x14); Start-Sleep -Milliseconds 600
[void][W]::SetForegroundWindow($h); Start-Sleep -Milliseconds 300

$m = Caption 3; Click $m[0] $m[1]; Start-Sleep -Milliseconds 800
Report "minimize button" ([W]::IsIconic($h)) ""
[void][W]::ShowWindow($h, 9); Start-Sleep -Milliseconds 800
[void][W]::SetForegroundWindow($h); Start-Sleep -Milliseconds 300
$m = Caption 1; Click $m[0] $m[1]; Start-Sleep -Milliseconds 1500
Report "close button ends the process" $p.HasExited ""
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
