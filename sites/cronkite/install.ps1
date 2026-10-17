# Cronkite installer for Windows (PKG-7). Installs or updates the whole suite, Brick, Slate,
# Frame, and Etch, for this user, without admin rights, by running each app's own installer
# from its site (https://<app>.cronkite.cc/install.ps1):
#
#   irm https://cronkite.cc/install.ps1 | iex
#   $env:CRONKITE_APPS = "slate frame"; irm https://cronkite.cc/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://cronkite.cc/install.ps1))) -Apps slate,frame
#   & ([scriptblock]::Create((irm https://cronkite.cc/install.ps1))) -Uninstall
#
# Each app's installer does the work and its own checks (64-bit Windows 10 or 11); this
# runs them one after another, keeps going past one that fails, and ends with each app's
# result, then fails (throws) if any app did. -Apps, or else $env:CRONKITE_APPS (names
# separated by commas or spaces), picks the apps; -Uninstall removes them, as Settings >
# Apps would, and keeps your settings and recent files.
# For tests: $env:CRONKITE_SITE_BASE = URL takes URL/<app>/install.ps1 in place of each
# app's site, and the apps' installers' own variables ($env:BRICK_DOWNLOAD_URL, …) pass
# through to them.
# Windows PowerShell 5.1 or PowerShell 7.
param([string[]]$Apps, [switch]$Uninstall)

& {
    param([string[]]$Apps, [bool]$Uninstall)
    $ErrorActionPreference = 'Stop'
    # Invoke-WebRequest's progress bar slows Windows PowerShell's downloads to a crawl.
    $ProgressPreference = 'SilentlyContinue'
    $all = 'brick', 'slate', 'frame', 'etch'
    $titles = @{ brick = 'Brick'; slate = 'Slate'; frame = 'Frame'; etch = 'Etch' }

    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
        throw "cronkite install: this installer is for Windows; on Linux and macOS: curl -fsSL https://cronkite.cc/install.sh | sh"
    }
    $asked = if ($Apps) { $Apps } elseif ($env:CRONKITE_APPS) { @($env:CRONKITE_APPS) } else { @() }
    $named = @($asked | ForEach-Object { $_ -split '[\s,]+' } | Where-Object { $_ } | ForEach-Object { $_.ToLowerInvariant() })
    foreach ($a in $named) {
        if ($all -notcontains $a) { throw "cronkite install: no app called $a (the apps: $($all -join ' '))" }
    }
    # The suite's order, each once.
    $chosen = @(if ($named) { $all | Where-Object { $named -contains $_ } } else { $all })
    $verb = if ($Uninstall) { 'removed' } else { 'installed' }

    # Windows PowerShell 5.1 may offer only older TLS by default.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $results = @()
    $failed = @()
    $i = 0
    foreach ($app in $chosen) {
        $i++
        $name = $titles[$app]
        $site = if ($env:CRONKITE_SITE_BASE) { "$($env:CRONKITE_SITE_BASE.TrimEnd('/'))/$app" } else { "https://$app.cronkite.cc" }
        Write-Host ""
        Write-Host "== $name ($i of $($chosen.Count)): $site/install.ps1"
        try {
            try { $text = Invoke-RestMethod -UseBasicParsing -Uri "$site/install.ps1" }
            catch { throw "cronkite install: cannot reach $site/install.ps1 ($($_.Exception.Message))" }
            if ($text -is [byte[]]) { $text = [Text.Encoding]::UTF8.GetString($text) }
            $installer = [scriptblock]::Create(([string]$text).TrimStart([char]0xFEFF))
            # Its own scope; what it writes goes to the screen, and what it throws is its failure.
            if ($Uninstall) { & $installer -Uninstall | Out-Host } else { & $installer | Out-Host }
            $results += "  {0,-6} {1}" -f $name, $verb
        } catch {
            Write-Host $_.Exception.Message -ForegroundColor Red
            $failed += $name
            $results += "  {0,-6} {1}" -f $name, 'failed (see above)'
        }
    }

    Write-Host ""
    Write-Host "Cronkite:"
    $results | ForEach-Object { Write-Host $_ }
    $n = $chosen.Count
    $line = "Cronkite: $($n - $failed.Count) of $n $(if ($n -eq 1) { 'app' } else { 'apps' }) $verb"
    if ($failed) { $line += "; failed: $($failed -join ', ')" }
    Write-Host $line
    if ($failed) { throw "cronkite install: $($failed -join ', ') failed (see above)" }
} $Apps $Uninstall
