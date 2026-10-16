# Frame installer for Windows (WIN-5). Installs or updates the latest release for this
# user, without admin rights:
#
#   irm https://frame.cronkite.cc/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://frame.cronkite.cc/install.ps1))) -Uninstall
#
# It reads latest.json's "platforms" "windows-x86_64" entry (url, sha256, bytes), downloads
# Frame's Setup.exe, checks it against the sha256, and runs it silently: Frame goes into
# %LOCALAPPDATA%\Programs\Frame with a Start menu entry and an entry in Settings > Apps, and
# PowerPoint presentations (.pptx .pptm) list it under Open with. Windows keeps your
# default apps. Files PowerShell downloads carry no mark of the web, so SmartScreen
# doesn't stop the unsigned Setup.exe.
# -Uninstall runs Frame's uninstaller silently, as Settings > Apps would; your settings and
# recent files (%APPDATA%\Cronkite\Frame, %LOCALAPPDATA%\Cronkite\Frame) stay.
# Windows 10 or 11 on x64; Windows PowerShell 5.1 or PowerShell 7.
param([switch]$Uninstall)

& {
    param([bool]$Uninstall)
    $ErrorActionPreference = 'Stop'
    # Invoke-WebRequest's progress bar slows Windows PowerShell's downloads to a crawl.
    $ProgressPreference = 'SilentlyContinue'
    # Where latest.json is: filled in when the page is deployed (scripts/ship.sh frame site).
    $base = if ($env:FRAME_DOWNLOAD_URL) { $env:FRAME_DOWNLOAD_URL } else { '@DOWNLOAD_BASE_URL@' }
    $name = 'Frame'
    $uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\dev.cronkite.Frame_is1'
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
        if (Test-Path $uninstallKey) { throw "frame install: $name's uninstaller didn't finish ($uninstaller)" }
        Write-Host "Removed $name. Your documents and its settings (%APPDATA%\Cronkite\$name, %LOCALAPPDATA%\Cronkite\$name) are untouched."
        return
    }

    if ([Environment]::OSVersion.Version.Major -lt 10 -or -not [Environment]::Is64BitOperatingSystem) {
        throw "frame install: $name needs 64-bit Windows 10 or 11"
    }
    if ($base.StartsWith('@')) { throw "frame install: no download URL set (FRAME_DOWNLOAD_URL)" }
    # Windows PowerShell 5.1 may offer only older TLS by default.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    try { $release = Invoke-RestMethod -UseBasicParsing -Uri "$base/latest.json" }
    catch { throw "frame install: cannot reach $base/latest.json ($($_.Exception.Message))" }
    $version = $release.version
    if (-not $version) { throw "frame install: latest.json is not a release" }
    $windows = $release.platforms.'windows-x86_64'
    if (-not $windows -or -not $windows.url -or -not $windows.sha256) { throw "frame install: $name $version has no Windows release yet" }

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("frame-install-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    $kept = $false
    try {
        $setup = Join-Path $tmp ([IO.Path]::GetFileName(([uri]$windows.url).AbsolutePath))
        Write-Host "Downloading $name $version for Windows"
        try { Invoke-WebRequest -UseBasicParsing -Uri $windows.url -OutFile $setup }
        catch { throw "frame install: cannot download $($windows.url) ($($_.Exception.Message))" }
        $got = (Get-FileHash -Algorithm SHA256 -Path $setup).Hash.ToLowerInvariant()
        if ($got -ne $windows.sha256.ToLowerInvariant()) {
            throw "frame install: checksum mismatch (expected $($windows.sha256), got $got); nothing installed"
        }
        $before = Installed
        $log = Join-Path $tmp 'setup.log'
        $p = Start-Process -FilePath $setup -ArgumentList ($quiet + '/CURRENTUSER', '/CLOSEAPPLICATIONS', "/LOG=`"$log`"") -Wait -PassThru
        if ($p.ExitCode -ne 0) {
            $kept = $true
            throw "frame install: Setup failed (exit code $($p.ExitCode)); its log: $log"
        }
        $now = Installed
        if (-not $now) { $kept = $true; throw "frame install: Setup finished but $name isn't registered; its log: $log" }
        if ($before -and $before -ne $now) { Write-Host "Updated $name $before -> $now" } else { Write-Host "Installed $name $now" }
        $dir = (Get-ItemProperty -Path $uninstallKey).InstallLocation.TrimEnd('\')
        Write-Host "  ${dir}: start it from the Start menu"
        Write-Host "  PowerPoint presentations (.pptx .pptm) list it under Open with; to make it their default,"
        Write-Host "  right-click a file > Open with > Choose another app > $name > Always"
    } finally {
        if (-not $kept) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue }
    }
} $Uninstall
