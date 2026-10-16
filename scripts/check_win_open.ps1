# WIN-4 acceptance on a Windows runner (ci.yml, job windows): each app opens its files
# from the command line, as Explorer's Open with and a registered file type start it, and
# dropped from Explorer; its own options still work; documents draw through fontconfig,
# in the bundled fonts where Windows has none; and the renderer GTK picks is logged.
#
#   powershell -File scripts/check_win_open.ps1 [-Bin target\debug] [-Out out\win-open] [-PathPrefix DIR]
#
# Needs Windows PowerShell 5.1 (WinForms and C# 5 through Add-Type), an interactive
# desktop (GitHub's windows-latest), the four apps built in -Bin, MSYS2's UCRT64 bin
# folder in -PathPrefix (their DLLs), and the fonts fetched into target\fonts
# (python scripts/fetch_fonts.py). The sample files sit in a folder whose name has spaces
# and letters outside ASCII, as a user's folders may. The environment is Explorer's: no
# HOME, no XDG_* variables.
#   1. Command line: `app FILE` with a relative name, from that folder, for each sample:
#      among them office-fonts.docx and office-fonts.pptx (scripts/office_fonts.py:
#      Calibri, Cambria, Arial, Times New Roman, Courier New, which Windows has) and the
#      same naming the bundled stand-ins (Carlito, Caladea, Liberation), which it hasn't;
#      each log names the fonts folder added.
#   2. Open with: `cmd /c start "" app.exe "C:\...\FILE"`, the command line Explorer's Open
#      with runs (`"app.exe" "%1"`), with a file name outside ASCII.
#   3. A registered type: a ProgID under HKCU with `"app.exe" "%1"` as its open command;
#      the file opens by its type through ShellExecute (Start-Process FILE, what a
#      double-click runs) when the extension has no UserChoice, else through the ProgID's
#      class (ShellExecuteEx with SEE_MASK_CLASSNAME). The keys are removed afterwards.
#   4. The apps' own options: a benchmark with the file named first and the option after
#      it (GLib on Windows used to hand the last arguments to GApplication).
#   5. Drag and drop: the app starts with no file; a small window of this script starts an
#      OLE drag of the shell's own data object for the file (SHCreateDataObject: Shell
#      IDList Array and CF_HDROP, as Explorer drags it), and SendInput carries the pointer
#      onto the app's window, rests, then crosses into another widget and lets go there
#      (where GDK's Win32 drop kept a refusal; WIN-3's Notes). The file must open; each
#      app's GDK_DEBUG=dnd output is in logs\<app>-drop.*.txt.
#   6. Look: Slate on office-fonts.docx, the stand-ins, and Markdown (Liberation Sans),
#      Frame's deck, and Brick's grid with Pango's Win32 font map
#      (PANGOCAIRO_BACKEND=win32) beside the apps' own (fontconfig); GSK_RENDERER=gl, =ngl,
#      and =vulkan beside GTK's own choice.
# Every window is screenshotted into $Out\shots and every app's stderr kept in $Out\logs;
# each check is a line of $Out\summary.txt. Ends with "check_win_open: OK" or "FAILED".
param(
    [string]$Bin = "target\debug",
    [string]$Out = "out\win-open",
    [string]$PathPrefix = ""
)
$ErrorActionPreference = "Continue"
$root = (Resolve-Path "$PSScriptRoot\..").Path
$Bin = (Resolve-Path $Bin).Path
New-Item -ItemType Directory -Force "$Out\shots", "$Out\logs", "$Out\files" | Out-Null
$Out = (Resolve-Path $Out).Path
$shots = "$Out\shots"; $logs = "$Out\logs"
$summary = "$Out\summary.txt"
Set-Content -Encoding utf8 $summary ""
$script:failed = $false

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Forms;

public static class Win {
    delegate bool EnumProc(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr p);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("shell32.dll")] static extern void SHChangeNotify(int e, uint f, IntPtr a, IntPtr b);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }

    // The visible top-level windows of process `pid` that have a title, as "handle<TAB>title".
    public static string[] Windows(int pid) {
        var found = new List<string>();
        EnumWindows(delegate (IntPtr h, IntPtr p) {
            uint owner;
            GetWindowThreadProcessId(h, out owner);
            if (owner == (uint)pid && IsWindowVisible(h)) {
                var s = new StringBuilder(512);
                GetWindowTextW(h, s, 512);
                if (s.Length > 0) found.Add(h.ToInt64() + "\t" + s);
            }
            return true;
        }, IntPtr.Zero);
        return found.ToArray();
    }

    public static void Front(long h) {
        ShowWindow(new IntPtr(h), 9);
        SetForegroundWindow(new IntPtr(h));
    }

    public static int[] Rect(long h) {
        RECT r;
        GetWindowRect(new IntPtr(h), out r);
        return new int[] { r.Left, r.Top, r.Right, r.Bottom };
    }

    // The window alone, drawn by PrintWindow (PW_RENDERFULLCONTENT), even if covered.
    public static void Shot(long h, string file) {
        int[] r = Rect(h);
        int w = r[2] - r[0], hh = r[3] - r[1];
        if (w <= 0 || hh <= 0) return;
        using (var bmp = new Bitmap(w, hh)) {
            using (var g = Graphics.FromImage(bmp)) {
                IntPtr dc = g.GetHdc();
                PrintWindow(new IntPtr(h), dc, 2);
                g.ReleaseHdc(dc);
            }
            bmp.Save(file, System.Drawing.Imaging.ImageFormat.Png);
        }
    }

    public static void Screen(string file) {
        Rectangle b = System.Windows.Forms.Screen.PrimaryScreen.Bounds;
        using (var bmp = new Bitmap(b.Width, b.Height)) {
            using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(b.Location, Point.Empty, b.Size);
            bmp.Save(file, System.Drawing.Imaging.ImageFormat.Png);
        }
    }

    // Tell the shell that file associations changed (SHCNE_ASSOCCHANGED).
    public static void AssociationsChanged() {
        SHChangeNotify(0x08000000, 0, IntPtr.Zero, IntPtr.Zero);
    }

    [DllImport("user32.dll")] static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool repaint);

    // Move and size a window (its frame, shadows included).
    public static void Place(long h, int x, int y, int w, int hh) {
        MoveWindow(new IntPtr(h), x, y, w, hh, true);
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct SHELLEXECUTEINFO {
        public int cbSize; public uint fMask; public IntPtr hwnd; public string lpVerb; public string lpFile;
        public string lpParameters; public string lpDirectory; public int nShow; public IntPtr hInstApp;
        public IntPtr lpIDList; public string lpClass; public IntPtr hkeyClass; public uint dwHotKey;
        public IntPtr hIcon; public IntPtr hProcess;
    }
    [DllImport("shell32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool ShellExecuteExW(ref SHELLEXECUTEINFO i);

    // Open `file` with the open verb of ProgID `cls` (SEE_MASK_CLASSNAME): what the shell
    // does for a file of a type registered to it. Whether the shell started it.
    public static bool OpenAs(string file, string cls) {
        var i = new SHELLEXECUTEINFO();
        i.cbSize = Marshal.SizeOf(typeof(SHELLEXECUTEINFO));
        i.fMask = 0x1; // SEE_MASK_CLASSNAME
        i.lpVerb = "open"; i.lpFile = file; i.lpClass = cls; i.nShow = 1;
        return ShellExecuteExW(ref i);
    }
}

[ComImport, Guid("00000121-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IOleDropSource {
    [PreserveSig] int QueryContinueDrag(int fEscapePressed, int grfKeyState);
    [PreserveSig] int GiveFeedback(int dwEffect);
}

// The drag's source as Explorer's is: it drops when the left button goes up.
[ComVisible(true)]
public class DropSource : IOleDropSource {
    public int QueryContinueDrag(int fEscapePressed, int grfKeyState) {
        if (fEscapePressed != 0) return 0x00040101; // DRAGDROP_S_CANCEL
        if ((grfKeyState & 1) == 0) return 0x00040100; // DRAGDROP_S_DROP: MK_LBUTTON is up
        return 0;
    }
    public int GiveFeedback(int dwEffect) { return 0x00040102; } // DRAGDROP_S_USEDEFAULTCURSORS
}

public static class Drag {
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public MOUSEINPUT mi; }
    [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    const uint MOVE = 0x1, LEFTDOWN = 0x2, LEFTUP = 0x4, ABSOLUTE = 0x8000;

    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] static extern int SHParseDisplayName(string name, IntPtr bindCtx, out IntPtr pidl, uint sfgaoIn, out uint sfgaoOut);
    [DllImport("shell32.dll")] static extern IntPtr ILClone(IntPtr pidl);
    [DllImport("shell32.dll")] static extern bool ILRemoveLastID(IntPtr pidl);
    [DllImport("shell32.dll")] static extern IntPtr ILFindLastID(IntPtr pidl);
    [DllImport("shell32.dll")] static extern int SHCreateDataObject(IntPtr pidlFolder, uint cidl, IntPtr[] apidl, IntPtr inner, ref Guid riid, out System.Runtime.InteropServices.ComTypes.IDataObject data);
    [DllImport("ole32.dll")] static extern int DoDragDrop(System.Runtime.InteropServices.ComTypes.IDataObject data, IOleDropSource source, int okEffects, out int effect);

    // The pointer to (x, y) on the primary screen, with `button` (LEFTDOWN, LEFTUP, or 0).
    static void Send(uint button, int x, int y) {
        Rectangle b = System.Windows.Forms.Screen.PrimaryScreen.Bounds;
        var i = new INPUT();
        i.type = 0;
        i.mi.dx = (int)(x * 65535.0 / (b.Width - 1));
        i.mi.dy = (int)(y * 65535.0 / (b.Height - 1));
        i.mi.dwFlags = MOVE | ABSOLUTE | button;
        SendInput(1, new INPUT[] { i }, Marshal.SizeOf(typeof(INPUT)));
    }

    // The shell's own data object for `path`, as Explorer drags it: Shell IDList Array
    // (which GTK reads as text/uri-list), CF_HDROP, and the shell's other formats.
    static System.Runtime.InteropServices.ComTypes.IDataObject ShellData(string path) {
        IntPtr pidl;
        uint attributes;
        int hr = SHParseDisplayName(path, IntPtr.Zero, out pidl, 0, out attributes);
        if (hr != 0) throw new Exception("SHParseDisplayName 0x" + hr.ToString("x8"));
        IntPtr folder = ILClone(pidl);
        ILRemoveLastID(folder);
        Guid iid = new Guid("0000010e-0000-0000-C000-000000000046");
        System.Runtime.InteropServices.ComTypes.IDataObject data;
        hr = SHCreateDataObject(folder, 1, new IntPtr[] { ILFindLastID(pidl) }, IntPtr.Zero, ref iid, out data);
        if (hr != 0) throw new Exception("SHCreateDataObject 0x" + hr.ToString("x8"));
        return data;
    }

    // Drag `path` from a small window centred on (sx, sy) and drop it at (bx, by): the window
    // starts an OLE drag of the shell's data object for the file when the button goes down
    // on it, and the pointer carries it to (x, y), rests, then moves to (bx, by) and lets
    // go. Returns DoDragDrop's result and the drop effect the target chose (0: refused,
    // 1: copy), then keeps the source alive 3 s more for the target to read it.
    public static string Run(string path, int sx, int sy, int x, int y, int bx, int by) {
        string result = "no drag started";
        var t = new Thread(delegate () {
            var form = new Form();
            form.FormBorderStyle = FormBorderStyle.None;
            form.StartPosition = FormStartPosition.Manual;
            form.Location = new Point(sx - 60, sy - 60);
            form.Size = new Size(120, 120);
            form.TopMost = true;
            form.ShowInTaskbar = false;
            form.BackColor = Color.DarkOrange;
            form.MouseDown += delegate (object s, MouseEventArgs e) {
                result = "drag started";
                try {
                    int effect;
                    int hr = DoDragDrop(ShellData(path), new DropSource(), 1 | 2 | 4, out effect);
                    result = "DoDragDrop 0x" + hr.ToString("x8") + ", effect " + effect;
                } catch (Exception ex) {
                    result = "failed: " + ex.Message;
                }
                var close = new System.Windows.Forms.Timer();
                close.Interval = 3000;
                close.Tick += delegate (object a, EventArgs b) { close.Stop(); form.Close(); };
                close.Start();
            };
            form.Shown += delegate (object s, EventArgs e) {
                var mover = new Thread(delegate () {
                    Thread.Sleep(700);
                    Send(0, sx, sy); Thread.Sleep(300);
                    Send(LEFTDOWN, sx, sy); Thread.Sleep(500);
                    for (int k = 1; k <= 30; k++) {
                        Send(0, sx + (x - sx) * k / 30, sy + (y - sy) * k / 30);
                        Thread.Sleep(40);
                    }
                    Thread.Sleep(800);
                    Send(0, bx, by); Thread.Sleep(500);
                    Send(LEFTUP, bx, by);
                });
                mover.IsBackground = true;
                mover.Start();
            };
            Application.Run(form);
        });
        t.SetApartmentState(ApartmentState.STA);
        t.Start();
        if (!t.Join(40000)) return "timeout (" + result + ")";
        return result;
    }
}
"@

function Say([string]$line) { Write-Host $line; Add-Content -Encoding utf8 $summary $line }
function Pass([string]$what) { Say "ok: $what" }
function Fail([string]$what) { Say "FAILED: $what"; $script:failed = $true }

# A visible window of a process whose title contains $text; waits up to $seconds. Returns
# the handle (0 if none) and leaves the process's titles in $script:titles.
function Wait-Window($procs, [string]$text, [int]$seconds = 40) {
    $deadline = (Get-Date).AddSeconds($seconds)
    do {
        $script:titles = @()
        foreach ($p in @(& $procs)) {
            foreach ($w in [Win]::Windows($p.Id)) {
                $h, $t = $w -split "`t", 2
                $script:titles += $t
                if ($t.IndexOf($text, [StringComparison]::OrdinalIgnoreCase) -ge 0) { return [long]$h }
            }
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    return 0
}

# Every running process of $app (started since $since).
function Of([string]$app, [datetime]$since) {
    return { Get-Process -Name $app -ErrorAction SilentlyContinue | Where-Object { $_.StartTime -ge $since } }.GetNewClosure()
}

# Stop every app, and the shell's "How do you want to open" dialog if a check raised one.
function Stop-Apps() {
    foreach ($a in "brick", "slate", "frame", "etch", "OpenWith") { Get-Process -Name $a -ErrorAction SilentlyContinue | Stop-Process -Force }
    Start-Sleep -Milliseconds 800
}

# Let the window lay out and draw, bring it to the front, and keep it as shots\$name.png.
function Shot([long]$h, [string]$name) {
    Start-Sleep -Seconds 3
    [Win]::Front($h)
    Start-Sleep -Milliseconds 800
    [Win]::Shot($h, "$shots\$name.png")
}

# Start $app with $argv in $dir, stderr and stdout to logs\$name.*; returns the process.
function Start-App([string]$app, [string[]]$argv, [string]$name, [string]$dir = $files) {
    $how = @{
        FilePath = "$Bin\$app.exe"; WorkingDirectory = $dir; PassThru = $true; NoNewWindow = $true
        RedirectStandardOutput = "$logs\$name.out.txt"; RedirectStandardError = "$logs\$name.err.txt"
    }
    if ($argv) { $how.ArgumentList = $argv }
    $p = Start-Process @how
    # Keep a handle now, or ExitCode stays empty once it has exited.
    $null = $p.Handle
    return $p
}

# Start $app on $argv, wait for a window titled with $expect, screenshot it as $name.
# Returns the milliseconds it took (0 if no window came).
function Open-Check([string]$app, [string[]]$argv, [string]$expect, [string]$name, [string]$what) {
    $t0 = Get-Date
    $p = Start-App $app $argv $name
    $h = Wait-Window ({ $p }.GetNewClosure()) $expect
    $ms = [math]::Round(((Get-Date) - $t0).TotalMilliseconds)
    if ($h) {
        Shot $h $name
        Pass "$what (window '$expect' after $ms ms)"
    } else {
        [Win]::Screen("$shots\$name-screen.png")
        Fail "$what (no window titled '$expect'; titles: $($script:titles -join ' | '); exited: $($p.HasExited))"
        $ms = 0
    }
    Stop-Apps
    return $ms
}

# The renderer GTK chose, from GSK_DEBUG=renderer's line in a log.
function Renderer([string]$name) {
    $line = Select-String -Path "$logs\$name.err.txt" -Pattern "Using renderer '(\w+)'" | Select-Object -First 1
    if ($line) { return $line.Matches[0].Groups[1].Value } else { return "none logged" }
}

# --- Setup ---------------------------------------------------------------------------------
if ($PathPrefix) { $env:PATH = "$PathPrefix;$env:PATH" }
# As Explorer starts them: no HOME and no XDG_* (MSYS2 and Git Bash set HOME).
foreach ($v in "HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "GSK_RENDERER", "PANGOCAIRO_BACKEND") {
    Remove-Item "env:$v" -ErrorAction SilentlyContinue
}
$env:GSK_DEBUG = "renderer"
# A cold fontconfig cache, as on a fresh install: the first start builds it.
foreach ($d in "$env:LOCALAPPDATA\fontconfig", "$env:USERPROFILE\.cache\fontconfig") {
    Remove-Item -Recurse -Force $d -ErrorAction SilentlyContinue
}

# Spaces and letters outside ASCII in the folder's name (é and 文件).
$u = [string][char]0x00E9 + [char]0x6587 + [char]0x4EF6
$files = "$Out\files\Cronkite r" + [char]0x00E9 + "sum" + [char]0x00E9 + " " + [char]0x6587 + [char]0x4EF6
New-Item -ItemType Directory -Force $files | Out-Null
$rows = @("id,name,city,zip,amount,date")
$rng = New-Object System.Random 11
foreach ($i in 0..1999) {
    $rows += "$i,Person $i,$(@('Paris', 'Austin', 'Kyoto', 'Lagos')[$rng.Next(4)]),0$($rng.Next(1000, 9999)),$('{0:F2}' -f ($rng.NextDouble() * 1000)),2026-$('{0:D2}' -f $rng.Next(1, 13))-$('{0:D2}' -f $rng.Next(1, 29))"
}
[IO.File]::WriteAllLines("$files\people list.csv", $rows)
Copy-Item "$root\apps\brick\crates\xlsx\tests\data\fixture.xlsx" "$files\book.xlsx"
[IO.File]::WriteAllText("$files\read me.md", "# Read me`n`nOpened on **Windows**, in Liberation Sans: the quick brown fox jumps over the lazy dog, and the sphinx of black quartz judges my vow while the line runs long enough to wrap.`n`n- one`n- two`n")
Copy-Item "$root\apps\slate\crates\docx\tests\data\fixture.docx" "$files\letter.docx"
python "$root\scripts\office_fonts.py" docx "$files\office-fonts.docx"
# The same document, and a deck of it, naming the Office fonts and the bundled stand-ins
# themselves (Carlito, Caladea, Liberation), which Windows doesn't have: those must draw
# in the bundled ones.
python "$root\scripts\office_fonts.py" docx "$files\stand-ins.docx" --stand-ins
python "$root\scripts\office_fonts.py" pptx "$files\office-fonts.pptx"
python "$root\scripts\office_fonts.py" pptx "$files\stand-ins.pptx" --stand-ins
Copy-Item "$root\apps\frame\crates\pptx\tests\data\fixture.pptx" "$files\deck.pptx"
Copy-Item "$root\apps\slate\crates\document-model\tests\data\picture.png" "$files\picture.png"
# One file per app named outside ASCII, for Explorer's launches.
$named = @{
    brick = "people $u.csv"; slate = "letter $u.docx"; frame = "deck $u.pptx"; etch = "picture $u.png"
}
Copy-Item "$files\people list.csv" "$files\$($named.brick)"
Copy-Item "$files\letter.docx" "$files\$($named.slate)"
Copy-Item "$files\deck.pptx" "$files\$($named.frame)"
Copy-Item "$files\picture.png" "$files\$($named.etch)"
Copy-Item "$files\office-fonts.docx" "$shots\office-fonts.docx"
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Say "screen $($screen.Width)x$($screen.Height); files in $files"
Say "fonts folder: $((Get-ChildItem "$root\target\fonts" -Filter *.ttf | Measure-Object).Count) .ttf files in $root\target\fonts"

# --- 1. Command line ---------------------------------------------------------------------
Say "== 1. command line (relative names, from the files' folder)"
$first = Open-Check brick @('"people list.csv"') "people list.csv" "brick-cmd-csv" "brick people list.csv (the first start, cold fontconfig cache)"
$second = Open-Check brick @('"book.xlsx"') "book.xlsx" "brick-cmd-xlsx" "brick book.xlsx"
Say "start to window: first $first ms (cold fontconfig cache), second $second ms"
Open-Check slate @('"read me.md"') "read me.md" "slate-cmd-md" "slate read me.md" | Out-Null
Open-Check slate @('"letter.docx"') "letter.docx" "slate-cmd-docx" "slate letter.docx" | Out-Null
Open-Check slate @('"office-fonts.docx"') "office-fonts.docx" "slate-cmd-office-fonts" "slate office-fonts.docx" | Out-Null
Open-Check slate @('"stand-ins.docx"') "stand-ins.docx" "slate-cmd-stand-ins" "slate stand-ins.docx (Carlito, Caladea, Liberation by name)" | Out-Null
Open-Check frame @('"deck.pptx"') "deck.pptx" "frame-cmd-pptx" "frame deck.pptx" | Out-Null
Open-Check frame @('"office-fonts.pptx"') "office-fonts.pptx" "frame-cmd-office-fonts" "frame office-fonts.pptx" | Out-Null
Open-Check frame @('"stand-ins.pptx"') "stand-ins.pptx" "frame-cmd-stand-ins" "frame stand-ins.pptx (Carlito, Caladea, Liberation by name)" | Out-Null
Open-Check etch @('"picture.png"') "picture.png" "etch-cmd-png" "etch picture.png" | Out-Null
foreach ($n in "brick-cmd-csv", "slate-cmd-docx", "frame-cmd-pptx", "etch-cmd-png") {
    $log = Get-Content -Raw "$logs\$n.err.txt" -ErrorAction SilentlyContinue
    if ($log -match "fonts: added (.*) to fontconfig in (\d+) ms") { Pass "$n added the bundled fonts ($($Matches[1]), $($Matches[2]) ms)" }
    else { Fail "$n didn't add the bundled fonts" }
    Say "$n renderer: $(Renderer $n)"
}
if ((Get-Content -Raw "$logs\slate-cmd-docx.err.txt") -match "isn't fontconfig's") { Fail "slate's font map isn't fontconfig's" }
else { Pass "slate's font map is fontconfig's (embedded fonts can show)" }
# Pango warns "couldn't load font X, falling back" for a family it can't find (DEC-11 R8).
function Missing([string]$name) {
    return @(Select-String -Path "$logs\$name.err.txt" -Pattern "couldn't load font" -ErrorAction SilentlyContinue)
}
foreach ($n in "slate-cmd-md", "slate-cmd-office-fonts", "slate-cmd-stand-ins", "frame-cmd-office-fonts", "frame-cmd-stand-ins") {
    $missing = Missing $n
    if ($missing.Count -eq 0) { Pass "$n found every font it asked for" }
    else { Fail "${n}: $($missing[0].Line.Trim())" }
}

# --- 2. Explorer's Open with ---------------------------------------------------------------
Say "== 2. Open with: cmd /c start `"`" app.exe `"FULL PATH`""
foreach ($app in "brick", "slate", "frame", "etch") {
    $file = "$files\$($named[$app])"
    $t0 = Get-Date
    # The whole line as cmd gets it: cmd.exe /c start "" "C:\...\app.exe" "C:\...\FILE".
    Start-Process -FilePath cmd.exe -WindowStyle Hidden -ArgumentList "/c start `"`" `"$Bin\$app.exe`" `"$file`""
    $h = Wait-Window (Of $app $t0) $named[$app]
    if ($h) { Shot $h "$app-open-with"; Pass "$app opens $($named[$app]) as Open with starts it" }
    else { [Win]::Screen("$shots\$app-open-with-screen.png"); Fail "$app via start: no window titled $($named[$app]) (titles: $($script:titles -join ' | '))" }
    Stop-Apps
}

# --- 3. A registered file type -------------------------------------------------------------
Say "== 3. a registered type: HKCU ProgID with `"app.exe`" `"%1`""
$classes = "HKCU:\Software\Classes"
foreach ($app in "brick", "slate", "frame", "etch") {
    $file = "$files\$($named[$app])"
    $ext = [IO.Path]::GetExtension($file)
    $progid = "Cronkite.Check.$app"
    $command = "$classes\$progid\shell\open\command"
    New-Item -Force $command | Out-Null
    Set-Item -Path $command -Value "`"$Bin\$app.exe`" `"%1`""
    $choice = Test-Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\$ext\UserChoice"
    $hadExt = Test-Path "$classes\$ext"
    $oldDefault = if ($hadExt) { (Get-Item "$classes\$ext").GetValue("") } else { $null }
    $t0 = Get-Date
    if ($choice) {
        $how = "the ProgID's class (the user chose another app for $ext)"
        [void][Win]::OpenAs($file, $progid)
    } else {
        $how = "its type, as a double-click does"
        if (-not $hadExt) { New-Item -Force "$classes\$ext" | Out-Null }
        Set-Item -Path "$classes\$ext" -Value $progid
        [Win]::AssociationsChanged()
        Start-Process -FilePath $file
    }
    $h = Wait-Window (Of $app $t0) $named[$app]
    if ($h) { Shot $h "$app-registered"; Pass "$app opens $($named[$app]) by $how" }
    else { [Win]::Screen("$shots\$app-registered-screen.png"); Fail "$app by $how`: no window titled $($named[$app]) (titles: $($script:titles -join ' | '))" }
    Stop-Apps
    if (-not $choice) {
        if ($hadExt) { Set-Item -Path "$classes\$ext" -Value $oldDefault } else { Remove-Item -Recurse -Force "$classes\$ext" }
    }
    Remove-Item -Recurse -Force "$classes\$progid"
    [Win]::AssociationsChanged()
}

# --- 4. The apps' own options --------------------------------------------------------------
Say "== 4. the apps' own options, after the file"
$benches = @(
    @("brick", @('"people list.csv"', "--bench-open")),
    @("slate", @('"letter.docx"', "--bench-flow")),
    @("frame", @('"deck.pptx"', "--bench-draw", "10")),
    @("etch", @("--bench-stroke", "10"))
)
foreach ($b in $benches) {
    $app = $b[0]; $argv = $b[1]
    $p = Start-App $app $argv "$app-bench"
    if (-not $p.WaitForExit(180000)) { Stop-Apps; Fail "$app $($argv -join ' '): still running after 180 s"; continue }
    $report = (Get-Content -Raw "$logs\$app-bench.out.txt" -ErrorAction SilentlyContinue)
    if ($p.ExitCode -eq 0 -and $report) { Pass "$app $($argv -join ' '): exit 0, $($report.Trim() -replace '\s+', ' ')" }
    else { Fail "$app $($argv -join ' '): exit $($p.ExitCode), stdout '$report', stderr $(Get-Content -Raw "$logs\$app-bench.err.txt")" }
}

# --- 5. Drag and drop ----------------------------------------------------------------------
Say "== 5. drag and drop onto the app's window"
$untitled = @{ brick = "Brick"; slate = "Slate"; frame = "Untitled"; etch = "Untitled" }
$dropped = @{ brick = "people list.csv"; slate = "letter.docx"; frame = "deck.pptx"; etch = "picture.png" }
# Each drag's last move, from the window's top-left corner (placed at 320,40; Etch's may
# be wider than asked): Brick's into the "Recent" heading, Slate's over the Open button's
# top edge, Frame's from the canvas into the slide list, Etch's from the tool column into
# the canvas.
$edges = @{
    brick = @(@(340, 300), @(343, 303)); slate = @(@(275, 334), @(275, 344))
    frame = @(@(260, 360), @(240, 360)); etch = @(@(50, 300), @(70, 300))
}
foreach ($app in "brick", "slate", "frame", "etch") {
    $t0 = Get-Date
    $env:GDK_DEBUG = "dnd"
    $p = Start-App $app @() "$app-drop"
    Remove-Item env:GDK_DEBUG
    $h = Wait-Window (Of $app $t0) $untitled[$app]
    if (-not $h) { Fail "$app without a file: no window '$($untitled[$app])' (titles: $($script:titles -join ' | '))"; Stop-Apps; continue }
    Start-Sleep -Seconds 2
    # The window on the right, the drag's source on the left, clear of its borders.
    [Win]::Place($h, 320, 40, 680, 600)
    [Win]::Front($h)
    Start-Sleep -Milliseconds 1500
    $r = [Win]::Rect($h)
    $left = $r[0]; $top = $r[1]
    # The drag rests, then its last move crosses into another widget and stops: there GTK
    # reports no action for that move and GDK's Win32 drop kept it (WIN-3's Notes).
    $a, $b = $edges[$app]
    $x = $left + $a[0]; $y = $top + $a[1]; $bx = $left + $b[0]; $by = $top + $b[1]
    [Win]::Screen("$shots\$app-drop-1-before.png")
    $effect = [Drag]::Run("$files\$($dropped[$app])", 120, 340, $x, $y, $bx, $by)
    $h2 = Wait-Window (Of $app $t0) $dropped[$app] 20
    if ($h2) { Shot $h2 "$app-drop-2-opened"; Pass "$app opens $($dropped[$app]) dropped on its window, last move $x,$y to $bx,$by (drop effect $effect)" }
    else { [Win]::Screen("$shots\$app-drop-2-screen.png"); Fail "${app}: dropping $($dropped[$app]) with the last move $x,$y to $bx,$by (effect $effect) opened nothing (titles: $($script:titles -join ' | '))" }
    Stop-Apps
}

# --- 6. Look: fonts and renderer -----------------------------------------------------------
Say "== 6. look: Pango's Win32 font map and other renderers beside the apps' own"
$env:PANGOCAIRO_BACKEND = "win32"
Open-Check slate @('"office-fonts.docx"') "office-fonts.docx" "slate-win32-office-fonts" "slate office-fonts.docx with PANGOCAIRO_BACKEND=win32" | Out-Null
Open-Check slate @('"stand-ins.docx"') "stand-ins.docx" "slate-win32-stand-ins" "slate stand-ins.docx with PANGOCAIRO_BACKEND=win32" | Out-Null
Open-Check slate @('"read me.md"') "read me.md" "slate-win32-md" "slate read me.md with PANGOCAIRO_BACKEND=win32" | Out-Null
Open-Check frame @('"stand-ins.pptx"') "stand-ins.pptx" "frame-win32-stand-ins" "frame stand-ins.pptx with PANGOCAIRO_BACKEND=win32" | Out-Null
Open-Check brick @('"people list.csv"') "people list.csv" "brick-win32-csv" "brick people list.csv with PANGOCAIRO_BACKEND=win32" | Out-Null
Remove-Item env:PANGOCAIRO_BACKEND
foreach ($n in "slate-win32-stand-ins", "slate-win32-md", "frame-win32-stand-ins") {
    $missing = Missing $n
    Say "${n}: $($missing.Count) fonts Pango's Win32 font map couldn't load$(if ($missing.Count) { ', first: ' + $missing[0].Line.Trim() })"
}
foreach ($r in "gl", "ngl", "vulkan") {
    $env:GSK_RENDERER = $r
    Open-Check slate @('"office-fonts.docx"') "office-fonts.docx" "slate-gsk-$r" "slate with GSK_RENDERER=$r" | Out-Null
    Say "GSK_RENDERER=$r`: renderer $(Renderer "slate-gsk-$r"); $((Select-String -Path "$logs\slate-gsk-$r.err.txt" -Pattern 'GL|Vulkan|renderer' | Select-Object -First 4 | ForEach-Object { $_.Line.Trim() }) -join ' / ')"
}
Remove-Item env:GSK_RENDERER

Stop-Apps
if ($script:failed) { Say "check_win_open: FAILED"; exit 1 }
Say "check_win_open: OK"
exit 0
