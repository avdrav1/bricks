# DEC-11 spike (throwaway): run each app's --version, then start it on a sample file for a
# few seconds, screenshot the desktop, and record whether it was still running and its
# window title. -Clean leaves only Windows' own folders on PATH (a bundle must carry its DLLs).
param(
    [Parameter(Mandatory)][string]$Bin,
    [Parameter(Mandatory)][string]$Out,
    [string]$PathPrefix = "",
    [int]$Seconds = 15,
    [switch]$Clean
)
$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$Bin = (Resolve-Path $Bin).Path
$root = (Resolve-Path "$PSScriptRoot\..\..\..").Path
Add-Type -AssemblyName System.Windows.Forms, System.Drawing

function Shot([string]$file) {
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

if ($Clean) {
    $env:PATH = "C:\Windows\System32;C:\Windows;C:\Windows\System32\WindowsPowerShell\v1.0"
    foreach ($v in "XDG_DATA_DIRS", "GDK_PIXBUF_MODULE_FILE", "GDK_PIXBUF_MODULEDIR", "GSETTINGS_SCHEMA_DIR", "PKG_CONFIG_PATH") {
        Remove-Item "env:$v" -ErrorAction SilentlyContinue
    }
}
if ($PathPrefix) { $env:PATH = "$PathPrefix;$env:PATH" }
# Ask GSK to log the renderer it picks (to the .err.txt files).
$env:GSK_DEBUG = "renderer"
# As started from Explorer or the Start menu: no HOME, no XDG_* (Git Bash and MSYS2 set HOME).
foreach ($v in "HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "XDG_CONFIG_HOME", "XDG_RUNTIME_DIR") {
    Remove-Item "env:$v" -ErrorAction SilentlyContinue
}
$work = Join-Path $Out "cwd"
New-Item -ItemType Directory -Force $work | Out-Null
Push-Location $work

$summary = "$Out\summary.txt"
"bin=$Bin clean=$Clean" | Out-File -Encoding utf8 $summary
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
"screen=$($b.Width)x$($b.Height)" | Out-File -Append -Encoding utf8 $summary

foreach ($app in "brick", "slate", "frame", "etch") {
    $exe = "$Bin\$app.exe"
    if (-not (Test-Path $exe)) { "${app}: no exe" | Out-File -Append -Encoding utf8 $summary; continue }
    $v = (& $exe --version 2>&1 | Out-String).Trim()
    "$app --version: exit=$LASTEXITCODE '$v'" | Out-File -Append -Encoding utf8 $summary
}

$runs = @(
    @("brick", "$PSScriptRoot\samples\sample.csv"),
    @("brick", "$root\apps\brick\crates\xlsx\tests\data\fixture.xlsx"),
    @("slate", "$root\README.md"),
    @("slate", "$root\apps\slate\crates\docx\tests\data\fixture.docx"),
    @("frame", "$root\apps\frame\crates\pptx\tests\data\fixture.pptx"),
    @("etch", "$PSScriptRoot\samples\sample.png"),
    @("brick", ""),
    @("frame", "")
)
foreach ($r in $runs) {
    $app = $r[0]; $file = $r[1]
    $exe = "$Bin\$app.exe"
    if (-not (Test-Path $exe)) { continue }
    $tag = if ($file) { "$app-" + [IO.Path]::GetFileName($file) } else { "$app-nofile" }
    $argv = @("`"$file`"")
    $start = Get-Date
    $p = if ($file) {
        Start-Process -FilePath $exe -ArgumentList $argv -PassThru `
            -RedirectStandardOutput "$Out\$tag.out.txt" -RedirectStandardError "$Out\$tag.err.txt"
    } else {
        Start-Process -FilePath $exe -PassThru `
            -RedirectStandardOutput "$Out\$tag.out.txt" -RedirectStandardError "$Out\$tag.err.txt"
    }
    $title = ""
    $deadline = $start.AddSeconds($Seconds)
    $shown = $null
    while ((Get-Date) -lt $deadline -and -not $p.HasExited) {
        Start-Sleep -Milliseconds 250
        $p.Refresh()
        if (-not $shown -and $p.MainWindowHandle -ne 0) { $shown = ((Get-Date) - $start).TotalMilliseconds }
    }
    $p.Refresh()
    $state = if ($p.HasExited) { "exited code=$($p.ExitCode)" } else { "running" }
    $title = $p.MainWindowTitle
    $ws = if ($p.HasExited) { "" } else { "ws=$([math]::Round($p.WorkingSet64 / 1MB))MB" }
    Shot "$Out\$tag.png"
    if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }
    Start-Sleep -Seconds 1
    "${tag}: $state window_after_ms=$shown title='$title' $ws" | Out-File -Append -Encoding utf8 $summary
}

# Slate's Original::sweep deletes package copies whose /proc/<pid> is missing: start Slate
# on a .docx, then a second Slate, and see whether the first one's copy survives.
$slate = "$Bin\slate.exe"
if (Test-Path $slate) {
    $cache = Join-Path ([IO.Path]::GetTempPath()) "slate"
    $a = Start-Process -FilePath $slate -ArgumentList "`"$root\apps\slate\crates\docx\tests\data\fixture.docx`"" -PassThru `
        -RedirectStandardOutput "$Out\sweep-a.out.txt" -RedirectStandardError "$Out\sweep-a.err.txt"
    Start-Sleep -Seconds 8
    $before = (Get-ChildItem $cache -ErrorAction SilentlyContinue | ForEach-Object Name) -join ","
    $b2 = Start-Process -FilePath $slate -ArgumentList "`"$root\README.md`"" -PassThru `
        -RedirectStandardOutput "$Out\sweep-b.out.txt" -RedirectStandardError "$Out\sweep-b.err.txt"
    Start-Sleep -Seconds 8
    $after = (Get-ChildItem $cache -ErrorAction SilentlyContinue | ForEach-Object Name) -join ","
    "slate sweep: first slate pid=$($a.Id) copies in $cache before second start: [$before] after: [$after]" |
        Out-File -Append -Encoding utf8 $summary
    foreach ($p in $a, $b2) { if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force } }
}

# Where the apps wrote their state (Brick's recent list without HOME lands under the cwd).
Pop-Location
"files under the apps' working directory ${work}:" | Out-File -Append -Encoding utf8 $summary
Get-ChildItem -Recurse -Force $work -ErrorAction SilentlyContinue |
    ForEach-Object { "  " + $_.FullName.Substring($work.Length) } | Out-File -Append -Encoding utf8 $summary
foreach ($d in "$env:LOCALAPPDATA\brick", "$env:LOCALAPPDATA\slate", "$env:LOCALAPPDATA\etch", "$env:LOCALAPPDATA\frame",
    "$env:USERPROFILE\.local", "$env:USERPROFILE\.cache", ([IO.Path]::GetTempPath() + "brick"),
    ([IO.Path]::GetTempPath() + "frame"), ([IO.Path]::GetTempPath() + "etch"), ([IO.Path]::GetTempPath() + "slate")) {
    if (Test-Path $d) {
        "exists: $d -> " + ((Get-ChildItem -Recurse -Force $d | ForEach-Object { $_.FullName.Substring($d.Length) }) -join " ") |
            Out-File -Append -Encoding utf8 $summary
    }
}
Get-Content $summary
