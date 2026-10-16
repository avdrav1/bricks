# Brick installer for Windows (WIN-5). Installs or updates the latest release for this
# user, without admin rights:
#
#   irm https://brick.cronkite.cc/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://brick.cronkite.cc/install.ps1))) -Uninstall
#
# It reads latest.json's "platforms" "windows-x86_64" entry (url, sha256, bytes), downloads
# Brick's Setup.exe, checks it against the sha256, and runs it silently: Brick goes into
# %LOCALAPPDATA%\Programs\Brick with a Start menu entry and an entry in Settings > Apps, and
# CSV, TSV, and Excel workbooks (.csv .tsv .xlsx .xlsm) list it under Open with. Windows
# keeps your default apps. Files PowerShell downloads carry no mark of the web, so
# SmartScreen doesn't stop the unsigned Setup.exe.
# -Uninstall runs Brick's uninstaller silently, as Settings > Apps would; your settings and
# recent files (%APPDATA%\Cronkite\Brick, %LOCALAPPDATA%\Cronkite\Brick) stay.
# Windows 10 or 11 on x64; Windows PowerShell 5.1 or PowerShell 7.
param([switch]$Uninstall)

& {
    param([bool]$Uninstall)
    $ErrorActionPreference = 'Stop'
    # Invoke-WebRequest's progress bar slows Windows PowerShell's downloads to a crawl.
    $ProgressPreference = 'SilentlyContinue'
    # Where latest.json is: filled in when the page is deployed (scripts/ship.sh brick site).
    $base = if ($env:BRICK_DOWNLOAD_URL) { $env:BRICK_DOWNLOAD_URL } else { '@DOWNLOAD_BASE_URL@' }
    $name = 'Brick'
    $uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\dev.cronkite.Brick_is1'
    $quiet = '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'

    function Installed {
        $k = Get-ItemProperty -Path $uninstallKey -ErrorAction SilentlyContinue
        if ($k) { return $k.DisplayVersion } else { return $null }
    }

    if ($Uninstall) {
        $k = Get-ItemProperty -Path $uninstallKey -ErrorAction SilentlyContinue
        if (-not $k) { Write-Host "$name isn't installed for this user."; return }
        $uninstaller = $k.UninstallString.Trim('"')
        Start-Process -FilePath $uninstaller -ArgumentList $quiet -Wait
        # The uninstaller hands over to a copy of itself in the temp folder: wait for it.
        $deadline = (Get-Date).AddSeconds(120)
        while ((Test-Path $uninstallKey) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
        if (Test-Path $uninstallKey) { throw "brick install: $name's uninstaller didn't finish ($uninstaller)" }
        Write-Host "Removed $name. Your documents and its settings (%APPDATA%\Cronkite\$name, %LOCALAPPDATA%\Cronkite\$name) are untouched."
        return
    }

    if ([Environment]::OSVersion.Version.Major -lt 10 -or -not [Environment]::Is64BitOperatingSystem) {
        throw "brick install: $name needs 64-bit Windows 10 or 11"
    }
    if ($base.StartsWith('@')) { throw "brick install: no download URL set (BRICK_DOWNLOAD_URL)" }
    # Windows PowerShell 5.1 may offer only older TLS by default.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    try { $release = Invoke-RestMethod -UseBasicParsing -Uri "$base/latest.json" }
    catch { throw "brick install: cannot reach $base/latest.json ($($_.Exception.Message))" }
    $version = $release.version
    if (-not $version) { throw "brick install: latest.json is not a release" }
    $windows = $release.platforms.'windows-x86_64'
    if (-not $windows -or -not $windows.url -or -not $windows.sha256) { throw "brick install: $name $version has no Windows release yet" }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("brick-install-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    $kept = $false
    try {
        $setup = Join-Path $tmp ([IO.Path]::GetFileName(([uri]$windows.url).AbsolutePath))
        Write-Host "Downloading $name $version for Windows"
        try { Invoke-WebRequest -UseBasicParsing -Uri $windows.url -OutFile $setup }
        catch { throw "brick install: cannot download $($windows.url) ($($_.Exception.Message))" }
        $got = (Get-FileHash -Algorithm SHA256 -Path $setup).Hash.ToLowerInvariant()
        if ($got -ne $windows.sha256.ToLowerInvariant()) {
            throw "brick install: checksum mismatch (expected $($windows.sha256), got $got); nothing installed"
        }
        $before = Installed
        $log = Join-Path $tmp 'setup.log'
        $p = Start-Process -FilePath $setup -ArgumentList ($quiet + '/CURRENTUSER', '/CLOSEAPPLICATIONS', "/LOG=`"$log`"") -Wait -PassThru
        if ($p.ExitCode -ne 0) {
            $kept = $true
            throw "brick install: Setup failed (exit code $($p.ExitCode)); its log: $log"
        }
        $now = Installed
        if (-not $now) { $kept = $true; throw "brick install: Setup finished but $name isn't registered; its log: $log" }
        if ($before -and $before -ne $now) { Write-Host "Updated $name $before -> $now" } else { Write-Host "Installed $name $now" }
        $dir = (Get-ItemProperty -Path $uninstallKey).InstallLocation.TrimEnd('\')
        Write-Host "  ${dir}: start it from the Start menu"
        Write-Host "  CSV, TSV, and Excel workbooks (.csv .tsv .xlsx .xlsm) list it under Open with; to make it a"
        Write-Host "  type's default, right-click a file > Open with > Choose another app > $name > Always"
    } finally {
        if (-not $kept) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
    }
} $Uninstall
