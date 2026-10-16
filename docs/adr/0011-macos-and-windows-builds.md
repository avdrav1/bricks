# ADR 0011: Building and Shipping the Suite on macOS and Windows

Status: Accepted (2026-10-15), macOS first; Windows follows under the same decision

Date: 2026-10-15

## Context

The suite builds, tests, and ships for x86_64 Linux only: CI on Ubuntu, release tarballs built in an Ubuntu 24.04 container, per-user curl installers, GUI checks under headless sway. The user is considering macOS and Windows builds. Spike DEC-11 (`spikes/dec-11/`, three slices) built, tested, and started every app on GitHub's Windows (Server 2025, x86_64) and macOS (macOS 26, Apple Silicon; Intel checked too) runners and inventoried the Linux-only code:

- **`spikes/dec-11/inventory/REPORT.md`**: 43 findings with file and line, and every toolkit-free crate checked against `x86_64-pc-windows-{gnu,msvc}` and `aarch64-apple-darwin`.
- **`spikes/dec-11/windows/REPORT.md`**: MSYS2 UCRT64 and MSVC with gvsbuild's GTK, a Fedora MinGW cross build, and Wine. Product changes it needed are kept as `experiments.patch`.
- **`spikes/dec-11/macos/REPORT.md`**: Homebrew GTK, a `dylibbundler` app bundle, and a DMG.

### What the spike found

GTK 4 itself builds and runs on both platforms (MSYS2 GTK 4.24.1, gvsbuild 4.22.4, Homebrew 4.24.1). The suite's own code is what breaks:

| | Windows | macOS |
| --- | --- | --- |
| Compiles as is | No: `suite-file` (`OsStrExt` in `recent.rs`) and `csv-engine` (the libc SIGBUS guard) stop every app; `data-model`, Etch's recovery, and Slate's fonts follow | Yes, everything; four perf test/bench targets use Linux's `posix_fadvise` |
| Starts | Brick yes; Slate, Frame, Etch exit: `suite_platform` accepts only Linux | Same |
| With 8 small experiments (34 lines) | All four build with MSYS2 and MSVC, start, and draw their files; also as a self-contained folder | All four open and draw their files with the platform check opened |
| Saving | **Every save reports failure after replacing the file**: `save_atomic` fsyncs the folder by opening it, which Windows refuses (41 test failures). Renaming over an open or memory-mapped file works (Brick's mapped source is fine) | Saves land correctly on APFS |
| Data safety | A second Slate deletes the first one's private `.docx` copy (`/proc/<pid>` liveness) | Same |
| Folders | Brick's state folder is relative to where it started (`HOME` unset); recent lists stay empty (they keep lines starting with `/`) | XDG folders under the home folder work, but aren't the Mac's `~/Library` |
| Look and feel | A console window opens beside each app; Liberation, Carlito, and Calibri are missing (`.docx` text falls back) | Cmd shortcuts do nothing (Control is hard-coded); only Etch puts its menus in the menu bar; dark mode isn't followed; fonts as Windows; CoreText clips the tops of Brick's grid text |
| Opening files from the desktop | Not exercised | Finder's open goes nowhere: the apps read the command line only, not `GApplication::open` |
| Renderer | On the GPU-less runner GTK uses cairo and draws correctly; the cross-built GTK (a 4.21 snapshot) drew black with GL | GL draws black windows on the runner VMs; cairo draws correctly. Real hardware not checked |
| Tests | 41 save failures, 4 recent-list, 2 CRLF fixtures (no `.gitattributes`), 2 mapped-file truncation (Windows refuses), 7 Unix-only test targets | 768 pass; 3 temp-path tests (`/private/var`), 4 targets don't compile (`posix_fadvise`) |
| Size | Self-contained: 61 DLLs, 77 MB unpacked (stripped), 32 MB zipped for all four apps; GTK runtime about 66 MB | Brick.app with 41 dylibs: 31 MB, DMG 12.5 MB |

Not exercised on either: GPU rendering on real hardware, HiDPI (Windows), input methods, drag and drop, printing, screen readers. The runner VMs' timings say nothing about performance.

## Decision

1. **Toolchains: build natively, on GitHub's runners.**
   - Windows: MSYS2 UCRT64 (`x86_64-pc-windows-gnu`, MSYS2's GTK 4) on `windows-latest`. It worked first time and packages GTK; MSVC with gvsbuild also works and stays the fallback. Cross-compiling from Linux is not a release path: Fedora's MinGW GTK is a development snapshot that drew black windows, and Wine misdraws fonts.
   - macOS: Homebrew GTK 4 on the oldest arm64 runner (`macos-14`), because Homebrew's libraries require the macOS they were built on. arm64 only; no universal binaries.
2. **`suite-platform` owns the differences** (CLAUDE.md already gives it OS integration):
   - its platform enum gains Windows and macOS;
   - per-user folders, native on each OS (`%LOCALAPPDATA%` and `%APPDATA%` on Windows, `~/Library/Application Support` and `~/Library/Caches` on macOS), with Linux keeping XDG;
   - process liveness for Slate's package copies (replacing `/proc`);
   - the primary modifier (Control, or Command on macOS) for every accelerator and key handler;
   - opening files from the desktop through `GApplication::open` (`HANDLES_OPEN`) in every app, which Linux benefits from too;
   - registering the metric-compatible fonts (Carlito, Caladea, Liberation) bundled with the apps.
   - *Done for Windows by WIN-4 (2026-10-16), in `suite-ui` as MAC-6 did for macOS:* Pango draws through fontconfig on Windows too (`PANGOCAIRO_BACKEND=fc`), with the bundled fonts as fontconfig application fonts, over Windows' own fonts. Pango's Win32 font map (DirectWrite) can't use fonts registered for the process, so Liberation Sans and Slate's embedded fonts fell back there (runner screenshots). Windows' own Calibri, Cambria, Arial, Times New Roman, and Courier New draw as themselves, laid out with unhinted widths (`unhinted_metrics`): Calibri, Arial, Times New Roman, and Courier New break lines as their stand-ins do on Linux; real Cambria wraps one word earlier than Caladea in one paragraph of the test document. Documents naming the stand-ins break exactly as on Linux. Files from the command line, Explorer's Open with, and registered types reach `GApplication::open` through `handle-local-options`: GLib on Windows reads the process's own command line, which the apps parse themselves.
3. **`suite-file` stays portable:**
   - fsync the folder only on Unix (`F_FULLFSYNC` on macOS);
   - recent lists keep paths as paths, not `/`-prefixed strings, with `\\?\` and `/private` normalised;
   - Unix permission modes and `OsStrExt` move behind `cfg(unix)`;
   - the repository gets a `.gitattributes` so fixtures keep their bytes.
4. **Engine crates stay toolkit- and OS-free.** `csv-engine`'s SIGBUS guard is Unix-only; on Windows the mapped file can't be truncated, so the guard isn't needed there. Its Windows behaviour gets a test of its own.
5. **Renderer:** keep GTK's default and verify it on real hardware first. If GL draws black on real Macs or Windows machines, the apps choose cairo there (unless `GSK_RENDERER` is set). That's one line per app, decided on evidence.
   - *Amended by MAC-5 (2026-10-15):* with no real Mac to verify GL on, and GL black again on the macOS 14 runner, macOS defaults to cairo (unless `GSK_RENDERER` is set) from one place, `suite_ui::renderer::MAC_DEFAULT`. Once GL draws on a real Apple Silicon Mac (`GSK_RENDERER=gl`), setting it to `None` gives GTK's default back.
   - *Amended by WIN-4 (2026-10-16):* Windows keeps GTK's default, from one place, `suite_ui::renderer::WINDOWS_DEFAULT` (`None`). MSYS2's GTK 4.24 tries GL through WGL, then EGL, and draws with cairo where neither works. On the GPU-less runner that's every launch, and every window draws right; `GSK_RENDERER=gl` and `=vulkan` fail to realize and fall back to cairo too, so no window draws black. DEC-11's black windows came from Fedora's GTK 4.21 snapshot, which isn't shipped. GL through WGL on real hardware is a hand check before release; if it draws black, `WINDOWS_DEFAULT = Some("cairo")`.
6. **Packaging:**
   - Windows: one per-user installer per app (Inno Setup, no admin rights), each bundling its GTK runtime (about 30 MB compressed), built as a GUI-subsystem program (no console), and registering its file types under `HKCU`. Signed with Authenticode (Azure Trusted Signing, or an OV certificate).
   - macOS: one `.app` per app (`dylibbundler` plus the GTK data: schemas, icons, pixbuf loaders), in a DMG, signed with a Developer ID, with the hardened runtime, and notarized ($99 a year).
   - Linux is unchanged. `ship.sh` grows a platform argument; each app's `latest.json` gains per-platform entries, and its download page offers the visitor's platform first.
   - *Done for macOS by MAC-7 (2026-10-16), as the user's decision 2 below has it:* each app's `.app` is ad-hoc signed, not Developer ID signed or notarized (`scripts/mac_bundle.sh`; `suite_platform::bundle` points GTK at the bundle's data). `release-macos.yml` builds it on macos-14 and checks it on a runner without Homebrew; `ship.sh APP build --macos`/`fetch-macos` bring it into the release, and `latest.json`'s `platforms` lists each platform, keeping Linux's fields at the top level for older installers. The curl installer registers the app with Launch Services but leaves the Mac's default apps alone.
   - *Done for Windows by WIN-5 (2026-10-16), as the user's decisions 1 and 2 below have it:* each app's self-contained folder (MSYS2's UCRT64 GTK in MSYS2's prefix layout, `scripts/win_bundle.sh`) in an unsigned per-user Inno Setup installer, `<app>-<version>-x86_64-windows-setup.exe` (`packaging/windows/setup.iss`), about 18–21 MB; `install.ps1` runs it silently. File types are offered through `OpenWithProgids`, `Applications\<app>.exe`, and `RegisteredApplications`; no default is taken. The bundles carry no GPL code: cairo without lzo and libtiff without jbig are rebuilt from MSYS2's own source packages (`scripts/win_libs.sh`), and GLib's `gdbus.exe` stays out (no D-Bus session bus on Windows). `release-windows.yml` builds and checks it; WIN-6 brings it into `ship.sh` and the pages.
7. **CI:** `ci.yml` gains Windows and macOS jobs running clippy and the tests (the GUI checks stay Linux-only). Release builds and signing run on the runners from tags. A smoke job on each platform starts each app with a sample file and keeps a screenshot, as the spike's workflows did.
8. **Mac and Windows conventions** make the apps feel native:
   - menus in the macOS menu bar;
   - Command shortcuts and Mac dialogs;
   - native title bars on macOS;
   - dark mode.

## Consequences

- **Order and effort** (from the spike's estimates, revised for what the runners proved):
  - An unsigned build of all four apps that runs and behaves like the Linux apps takes about 2 days per platform: the compile fixes, the platform check, folder fsync, Slate's liveness, recent lists, folders, no console, `.gitattributes`.
  - Signed installers and bundles with CI add about a week per platform.
  - Mac and Windows conventions (Command keys, the menu bar, Finder opens, fonts, dark mode) take about a week more for macOS and a few days for Windows.
  - In all, about 3 weeks for macOS and 2 to 3 weeks for Windows.
- **Costs:** an Apple Developer account ($99 a year, needed for notarization) and Windows code signing (Azure Trusted Signing about $10 a month, or an OV certificate yearly). Unsigned builds work but meet Gatekeeper and SmartScreen warnings.
- **Size:** each app bundles GTK: about 30 MB to download on Windows and 12 to 15 MB on macOS, against 0.5 to 2 MB on Linux, where the system has GTK.
- **Testing:** the GUI checks (`check_*.sh`, docker and sway) stay Linux-only. The other platforms get tests and smoke screenshots in CI, so platform regressions in interaction (keys, menus, dialogs, input methods) need hand checks before releases.
- **Revisit if:** GL draws black on real hardware (renderer choice), GTK's macOS backend can't give native menus well enough (the Mac conventions stories), or bundle sizes matter (a shared GTK runtime on Windows instead of one per app).

## The user's decisions (2026-10-15)

1. **macOS first**, then Windows.
2. **An unsigned preview first.** Each app is ad-hoc signed (`codesign -s -`, free; Apple Silicon runs nothing unsigned).
   - The suite's usual curl installer puts it in `~/Applications`. curl doesn't set the quarantine attribute, so Gatekeeper doesn't stop it.
   - A DMG downloaded in a browser is quarantined. Its download page says how to allow it: on macOS 15 and later, System Settings → Privacy & Security → Open Anyway (right-click → Open no longer does it), or `xattr -dr com.apple.quarantine`.
   - Developer ID signing and notarization come later, once there's an Apple Developer account.
3. **Native folders:**
   - settings, state, and recovery in `~/Library/Application Support/<App>`;
   - caches in `~/Library/Caches/<App>`;
   - Linux keeps XDG.
4. **Mac-like windows:**
   - menus in the macOS menu bar, with the app menu (About, Hide, Quit);
   - Command shortcuts;
   - native title bars instead of GNOME-style header bars.
5. **All four apps** ship for macOS in the first release.
6. **Minimum OS:**
   - macOS 14 or later, which follows from the build runner;
   - for Windows, 10 or later is expected; confirm it against MSYS2's GTK 4 when the Windows work starts.

## The user's decisions for Windows (2026-10-16)

1. **An unsigned preview first.** A Setup.exe downloaded in a browser meets SmartScreen ("Windows protected your PC" → More info → Run anyway); the download page says so. The PowerShell one-liner isn't blocked, since `irm` sets no Mark-of-the-Web. Authenticode signing comes later.
2. **Installers:** one per-user Inno Setup `Setup.exe` per app (no admin rights, into `%LOCALAPPDATA%\Programs\<App>`, uninstalled from Settings → Apps), plus `irm https://<app>.cronkite.cc/install.ps1 | iex`, which runs the same Setup silently. Windows doesn't let programs take default apps, so the installers register the file types for Open with and leave the defaults alone.
3. **Windows-like windows:** Windows' own title bar instead of GTK header bars, the menus in a menu bar row under it, the toolbar row as on macOS, and Windows' light and dark mode followed. Shortcuts stay Control.
4. **All four apps**, Windows 10 and 11 on x64. The work stops at the XP2 gate; releasing waits for the user.

Folders follow decision 2 above (not asked again): settings and recent files in `%APPDATA%\Cronkite\<App>` (roaming); state, recovery, and caches in `%LOCALAPPDATA%\Cronkite\<App>`.
