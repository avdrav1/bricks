# PKG-7 acceptance on a clean Windows runner (install-suite.yml): cronkite.cc's install.ps1
# installs the whole suite per user by running each app's own install.ps1, or only the
# apps named; keeps going past one app's failure and reports each app's result; and
# uninstalls them all.
#
#   powershell -File scripts\check_win_install_suite.ps1 [-Out out\win-install-suite]
#
# Windows PowerShell 5.1. A throwaway suite site on 127.0.0.1 (HttpListener) serves
# sites\cronkite\install.ps1 and each app's sites\<app>\install.ps1 under <app>/
# ($env:CRONKITE_SITE_BASE), which download the live releases ($env:BRICK_DOWNLOAD_URL and
# the others, from -Dl, default https://dl.cronkite.cc). Each run is a fresh Windows
# PowerShell, as the page says:
#   1. $env:CRONKITE_APPS = "slate paint"; irm …/install.ps1 | iex: refused, nothing
#      installed;
#   2. & ([scriptblock]::Create((irm …/install.ps1))) -Apps slate,frame: exactly Slate and
#      Frame in Settings > Apps (their HKCU Uninstall keys);
#   3. $env:CRONKITE_APPS = "brick etch" with Etch's download URL pointing nowhere: Brick
#      installed, Etch failed and reported, a failing exit;
#   4. irm …/install.ps1 | iex: all four in Settings > Apps at the live versions (a
#      screenshot of Settings > Apps), each bin\<app>.exe in %LOCALAPPDATA%\Programs;
#   5. & ([scriptblock]::Create((irm …/install.ps1))) -Uninstall: all four removed, their
#      keys and folders gone.
# Transcripts land in $Out\logs, the screenshot in $Out\shots, the result lines in
# $Out\summary.txt; it ends with "check_win_install_suite: OK" or exits 1.
param(
    [string]$Out = "out\win-install-suite",
    [string]$Dl = "https://dl.cronkite.cc"
)
$ErrorActionPreference = "Continue"
$ProgressPreference = "SilentlyContinue"
$root = (Resolve-Path "$PSScriptRoot\..").Path
foreach ($d in "shots", "logs", "site") { New-Item -ItemType Directory -Force "$Out\$d" | Out-Null }
$Out = (Resolve-Path $Out).Path
$shots = "$Out\shots"; $logs = "$Out\logs"; $site = "$Out\site"
$summary = "$Out\summary.txt"
Set-Content -Encoding utf8 $summary ""
$script:failed = $false
function Note([string]$line) { Add-Content -Encoding utf8 $summary $line; Write-Host $line }
function Check([bool]$ok, [string]$what) { if ($ok) { Note "ok   $what" } else { Note "FAIL $what"; $script:failed = $true } }

$apps = 'brick', 'slate', 'frame', 'etch'
$names = @{ brick = 'Brick'; slate = 'Slate'; frame = 'Frame'; etch = 'Etch' }
function Dl([string]$app) { if ($app -eq 'brick') { $Dl } else { "$Dl/$app" } }
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
$versions = @{}
foreach ($a in $apps) { $versions[$a] = (Invoke-RestMethod -UseBasicParsing "$(Dl $a)/latest.json").version }
Note "live releases: $(($apps | ForEach-Object { "$($names[$_]) $($versions[$_])" }) -join ', ')"

function Key([string]$app) { "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\dev.cronkite.$($names[$app])_is1" }
# The apps Settings > Apps lists, by name, in the suite's order.
function Listed { @($apps | Where-Object { Test-Path (Key $_) } | ForEach-Object { $names[$_] }) -join ' ' }

Copy-Item "$root\sites\cronkite\install.ps1" "$site\install.ps1"
foreach ($a in $apps) {
    New-Item -ItemType Directory -Force "$site\$a" | Out-Null
    Copy-Item "$root\sites\$a\install.ps1" "$site\$a\install.ps1"
}
$port = 8766
$base = "http://127.0.0.1:$port"
$server = Start-Job -ArgumentList $site, $port -ScriptBlock {
    param($root, $port)
    $l = New-Object Net.HttpListener
    $l.Prefixes.Add("http://127.0.0.1:$port/")
    $l.Start()
    while ($l.IsListening) {
        $c = $l.GetContext()
        $path = Join-Path $root ([Uri]::UnescapeDataString($c.Request.Url.AbsolutePath.TrimStart('/')).Replace('/', '\'))
        if (Test-Path -PathType Leaf $path) {
            $c.Response.ContentType = "text/plain; charset=utf-8"
            $b = [IO.File]::ReadAllBytes($path)
            $c.Response.ContentLength64 = $b.Length
            $c.Response.OutputStream.Write($b, 0, $b.Length)
        } else { $c.Response.StatusCode = 404 }
        $c.Response.Close()
    }
}
$deadline = (Get-Date).AddSeconds(30)
do {
    try { $up = (Invoke-WebRequest -UseBasicParsing "$base/install.ps1").StatusCode -eq 200 } catch { $up = $false }
    if (-not $up) { Start-Sleep -Milliseconds 500 }
} while (-not $up -and (Get-Date) -lt $deadline)
Check $up "the suite site answers on $base"

$env:CRONKITE_SITE_BASE = $base
foreach ($a in $apps) { Set-Item "env:$($a.ToUpperInvariant())_DOWNLOAD_URL" (Dl $a) }
Check ((Listed) -eq '') "Settings > Apps lists no Cronkite app to start with"

# Run $cmd (with $url for the suite's install.ps1) in a fresh Windows PowerShell; its
# transcript goes to $logs\$tag.txt.
function Suite([string]$tag, [string]$cmd) {
    $cmd = $cmd.Replace('URL', "$base/install.ps1")
    Note "${tag}: $cmd"
    $text = & powershell.exe -NoProfile -Command $cmd 2>&1 | Out-String
    $code = $LASTEXITCODE
    Set-Content -Encoding utf8 "$logs\$tag.txt" $text
    $text -split "`r?`n" | ForEach-Object { Write-Host "     ${tag}: $_" }
    return @{ text = $text; code = $code }
}
function Has($r, [string]$pattern) { return $r.text -match "(?m)^$pattern\r?$" }
function Esc([string]$s) { [regex]::Escape($s) }

$env:CRONKITE_APPS = 'slate paint'
$r = Suite 'bad-name' 'irm URL | iex'
Check ($r.code -ne 0 -and $r.text -match 'no app called paint' -and (Listed) -eq '') "an unknown app is refused (exit $($r.code)) and nothing installed"
Remove-Item env:CRONKITE_APPS

$r = Suite 'subset' '& ([scriptblock]::Create((irm URL))) -Apps slate,frame'
Check ($r.code -eq 0 -and (Has $r "Installed Slate $(Esc $versions.slate)") -and (Has $r "Installed Frame $(Esc $versions.frame)") -and (Has $r 'Cronkite: 2 of 2 apps installed')) "-Apps slate,frame installs Slate and Frame (exit $($r.code))"
Check ((Listed) -eq 'Slate Frame') "Settings > Apps lists only Slate and Frame: $(Listed)"

$env:CRONKITE_APPS = 'brick etch'
$env:ETCH_DOWNLOAD_URL = "$base/nowhere"
$r = Suite 'one-fails' 'irm URL | iex'
Check ($r.code -ne 0 -and (Has $r "Installed Brick $(Esc $versions.brick)") -and $r.text -match "etch install: cannot reach $(Esc "$base/nowhere/latest.json")" -and (Has $r '  Etch   failed \(see above\)') -and (Has $r 'Cronkite: 1 of 2 apps installed; failed: Etch')) "with Etch's release unreachable, Brick installs and Etch's failure is reported (exit $($r.code))"
Remove-Item env:CRONKITE_APPS
$env:ETCH_DOWNLOAD_URL = Dl 'etch'

$r = Suite 'all' 'irm URL | iex'
Check ($r.code -eq 0 -and (Has $r 'Cronkite: 4 of 4 apps installed')) "irm | iex installs all four (exit $($r.code))"
foreach ($a in $apps) {
    $n = $names[$a]
    $k = Get-ItemProperty (Key $a) -ErrorAction SilentlyContinue
    $exe = "$env:LOCALAPPDATA\Programs\$n\bin\$a.exe"
    Check ($k -and $k.DisplayName -eq $n -and $k.DisplayVersion -eq $versions[$a] -and (Test-Path $exe) -and (Has $r "  $(Esc ('{0,-6}' -f $n)) installed")) "Settings > Apps lists $($k.DisplayName) $($k.DisplayVersion); $exe is there"
}
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Start-Process "ms-settings:appsfeatures"
Start-Sleep -Seconds 10
$screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($screen.Location, [System.Drawing.Point]::Empty, $screen.Size)
$bmp.Save("$shots\settings-apps.png", [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Get-Process -Name SystemSettings -ErrorAction SilentlyContinue | Stop-Process -Force

$r = Suite 'uninstall' '& ([scriptblock]::Create((irm URL))) -Uninstall'
Check ($r.code -eq 0 -and (Has $r 'Cronkite: 4 of 4 apps removed')) "-Uninstall removes all four (exit $($r.code))"
Check ((Listed) -eq '') "Settings > Apps lists no Cronkite app: '$(Listed)'"
$left = @($apps | Where-Object { Test-Path "$env:LOCALAPPDATA\Programs\$($names[$_])" })
Check ($left.Count -eq 0) "no app folder left in %LOCALAPPDATA%\Programs: '$($left -join ' ')'"

Stop-Job $server -ErrorAction SilentlyContinue
if ($script:failed) { Note "check_win_install_suite: FAILED"; exit 1 }
Note "check_win_install_suite: OK"
