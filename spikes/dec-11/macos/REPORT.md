# DEC-11 spike: macOS

What breaks building and running Cronkite's four apps on macOS, measured on GitHub's
macOS runners. Static Linux-only code is inventoried in `spikes/dec-11/inventory/REPORT.md`
(findings F1-F43); this report confirms or refutes the macOS ones at run time and adds what
only a Mac shows.

Probe: `.github/workflows/spike-macos.yml` runs `spikes/dec-11/macos/ci/run.sh <phase>`;
each phase logs to `out/logs` and screenshots to `out/shots` (the run's artifact). Product
source is untouched; the one experiment (`ci/allow-macos.patch`, see C1) is applied in CI only.

| Run | What | URL |
|---|---|---|
| 1 | arm64 + Intel: check, first test attempt (stopped at compile errors), Intel Brick | https://github.com/avdrav1/cronkite/actions/runs/38781402667 |
| 2 | arm64 full: check, clippy, tests target by target, release builds, every app started (default renderer and cairo), patched apps, benches, keys, sweep, SIGBUS, `.app`/LaunchServices, dylibbundler, DMG, minos; Intel: check, Brick | https://github.com/avdrav1/cronkite/actions/runs/38783201936 |
| 3 | arm64: Brick.app with Homebrew hidden, fonts (CoreText vs fontconfig), GL debug, menus, typing, clipboard, dark mode | https://github.com/avdrav1/cronkite/actions/runs/38787324733 |
| 4 | arm64: typing with clicks and spacing, saving, external paste, Etch's File menu | https://github.com/avdrav1/cronkite/actions/runs/38789717490 |

Compact logs from these runs are in `logs/`; screenshots in `shots/`.

## Environment

| | arm64 (`macos-latest`) | x86_64 (`macos-15-intel`) |
|---|---|---|
| macOS | 26.6.2 (25G83), Apple M1 (Virtual), 3 CPUs, 7 GB | 15.7.9 |
| Display | 1024×768, scale 1 (no Retina) | 1920×1080, scale 1 |
| Rust | 1.99.1 stable, `aarch64-apple-darwin` | 1.99.1, `x86_64-apple-darwin` |
| Homebrew | 7.0.9; `brew install gtk4 pkgconf adwaita-icon-theme hicolor-icon-theme`: 30-35 s (bottles) | same formulae |
| GTK stack | gtk4 4.24.1, glib 2.90.1, pango 1.58.2, cairo 1.18.6, fontconfig 2.18.3, harfbuzz 14.6.0 | gtk4 4.22.4 |
| GSK renderers built | cairo, opengl/gl (Apple CGL, OpenGL 4.1 core); Vulkan and Broadway disabled in the bottle | same |
| Fonts | 456 families (CoreText's system fonts); no Liberation, Carlito, Caladea, DejaVu, Calibri. fontconfig: `Liberation Sans`→Arial, `Liberation Serif`→Times New Roman, `Liberation Mono`→Courier New, `Carlito`/`Calibri`→Verdana, `sans-serif`→Verdana, `monospace`→Andale Mono | — |
| Session | logged-in Aqua session; `screencapture` allowed (`CGPreflightScreenCaptureAccess` true); System Events (`osascript` keystrokes, menus, appearance) allowed | same |

## Build matrix

✓ works, ✗ fails, — not run. "Run" = the release binary started with a sample file for 8-10 s
and screenshotted.

| Crate / app | `cargo check` | `check --all-targets` / clippy `-D warnings` | Tests (lib, bins, tests, doc; one target at a time) | Release build | Run |
|---|---|---|---|---|---|
| suite-core, suite-ui, suite-platform | ✓ | ✓ | ✓ | — | — |
| suite-file | ✓ | ✓ | ✗ 7 passed, **3 failed** (`recent::tests::*`, C7) | — | — |
| csv-engine | ✓ | ✗ `tests/fetch_perf.rs`, `tests/index_perf.rs` don't compile (C6) | lib ✓ (incl. `source::tests::truncation_while_mapped_reports_changed_instead_of_crashing`, F2); 2 perf targets ✗ compile | — | — |
| data-model, formula, grid, commands, file-format, jobs, xlsx | ✓ | ✓ | ✓ | — | — |
| document-model, docx, flow, document-edit | ✓ | ✓ | ✓ | — | — |
| presentation-model, pptx, deck-edit | ✓ | ✓ | ✓ | — | — |
| image-model, image-io | ✓ | ✓ | ✓ | — | — |
| **brick** | ✓ | ✗ `tests/first_rows.rs`, `benches/corpus.rs` (C6) | bin 25 ✓; GUI perf tests `#[ignore]`d; `first_rows` ✗ compile | ✓ 141 s cold, 5.2 MB | ✓ with cairo (C2); xlsx ✓ |
| **slate** | ✓ | ✓ | bin 20 ✓ | ✓ | ✗ refuses (C1); ✓ with the patch and cairo |
| **frame** | ✓ | ✓ | ✓ | ✓ | ✗ refuses (C1); ✓ with the patch and cairo |
| **etch** | ✓ | ✓ | bin 42 ✓ | ✓ | ✗ refuses (C1); ✓ with the patch and cairo |

- Whole workspace: `cargo check --workspace` 70 s cold on arm64 (167 s on Intel), exit 0 on
  both. Tests: 170 targets, 166 exit 0; **768 passed, 3 failed, 69 ignored**; 3 targets don't
  compile. Clippy with `-D warnings` reports nothing beyond those compile errors: the product
  code is warning-free on macOS.
- Release builds (arm64, cold cache, `lto = "thin"`, `codegen-units = 1`): brick 141 s, then
  slate 34 s, frame 19 s, etch 28 s (deps shared). Binaries: brick 5.2 MB; patched slate
  4.1 MB, frame 2.7 MB, etch 2.3 MB. Unpatched slate/frame/etch are 0.7 MB: LTO folds
  `suite_platform::current()` to an error for `"macos"` and drops everything after it.
- Binaries are linker-signed ad hoc (`flags=0x20002(adhoc,linker-signed)`), thin arm64,
  `minos 11.0`.
- `--version` prints `brick 0.3.0`, `slate 0.3.0`, `frame 0.1.0`, `etch 0.2.1`.
- Intel: `cargo check --workspace` ✓, same 4 compile errors in `--all-targets`; Brick builds,
  starts, and shows the CSV with cairo, black with the default renderer, as on arm64
  (`shots/x86_64-*`).

## Failures by cause

| # | Cause | Affected | Evidence | Likely fix | Effort |
|---|---|---|---|---|---|
| C1 | `suite_platform::current()` accepts only `"linux"`; the apps exit at once | slate, frame, etch (Brick doesn't call it) | `slate: unsupported operating system "macos"; only Linux is supported`, exit 1 (`logs/run2-arm64-slate-refuses.txt`). With `ci/allow-macos.patch` (`"linux" \| "macos" => Ok(Platform::Linux)`) all three start | add `Platform::MacOs` and accept it | S |
| C2 | GTK's default renderer (`GskGLRenderer`, Apple CGL "Generic Float", OpenGL 4.1) draws every window **black** on the runner VMs | all four apps, both archs | `shots/arm64-default-renderer-all-black.png`; `logs/run3-arm64-gl-debug.txt`. `GSK_RENDERER=cairo` draws everything correctly. `gl`/`opengl` black; `vulkan` "Unrecognized renderer" (not built by Homebrew) | most likely the VM's GPU (no hardware OpenGL) [INFERENCE]: needs one run on a real Mac. If GL also fails there, set `GSK_RENDERER=cairo` on macOS before GTK starts. GTK has no Metal renderer; Apple's OpenGL is deprecated | S (verify) |
| C3 | Finder/LaunchServices opens arrive as an Apple Event (`GApplication::open`); the apps only read argv and don't set `HANDLES_OPEN` (inventory F34) | all four | `open -a Slate.app readme.md` shows the empty start window (`shots/arm64-slate-app-open-a-file.png`); a second `open -a Slate.app fixture.docx` while it runs goes to the same process and is dropped too (one window listed, `logs` run 2 `windows-app-2.txt`); `open -n -a Slate.app --args readme.md` opens the file (`shots/arm64-slate-app-open-args.png`) | `HANDLES_OPEN` + `connect_open` into each app's open path; LaunchServices gives one process per app, so `open` must make a window in the running process (Brick, Slate, Etch already do multi-window) | M |
| C4 | Shortcuts are bound to `<Control>` / `CONTROL_MASK` (F28-F30) | all four | Slate: Cmd+Z does nothing, Ctrl+Z undoes; Cmd+O nothing, Ctrl+O opens the file dialog (`shots/arm64-slate-cmd-z.png`, `-ctrl-z.png`, `-ctrl-o-native-dialog.png`). Etch's global File menu lists ⌃N ⌃O ⌃S ⌃⇧S ⌃⇧E ⌃W ⌃Q (`shots/arm64-etch-file-menu-global.png`); the app menus' Quit shows ⌃Q | `<Primary>` in accelerators (GTK maps it to ⌘ on macOS) and one `suite_ui` primary-modifier mask for hand-written key handlers; Option+arrows for word moves | M |
| C5 | Slate's `Original::sweep` treats a pid without `/proc/<pid>` as dead (F13) | slate | Slate A (pid 42660) running with `original-42660-0.docx` in `~/.cache/slate`; starting Slate B deletes it while A still runs (run 2 summary: "A alive: yes; cache now: " empty). A's next save of that .docx has no package to patch | liveness from the recovery store's owner lock (`suite_file::recovery`, which works on macOS, see What runs) or `kill(pid, 0)` | S |
| C6 | `libc::posix_fadvise` / `POSIX_FADV_DONTNEED` don't exist on macOS | csv-engine `tests/fetch_perf.rs:37`, `tests/index_perf.rs:39`; brick `tests/first_rows.rs:25`, `benches/corpus.rs:62` | `error[E0425]: cannot find function posix_fadvise in crate libc` (`logs/run2-arm64-posix_fadvise.txt`) | `#[cfg(target_os = "linux")]` on the cache drop (macOS: `fcntl(F_NOCACHE)` or `purge`) | S |
| C7 | Tests compare canonical paths with `std::env::temp_dir()`, which on macOS is `/var/folders/…`, a symlink to `/private/var/folders/…` | suite-file `recent::tests::{two_windows_keep_each_others_files, the_last_ten_files_survive_a_restart, opened_and_saved_images_list_once_newest_first_ten_at_most}` | `left: ["/private/var/folders/…/b.csv", …] right: ["/var/folders/…/b.csv", …]` | canonicalize the expected paths in the tests (product behaviour is right: the list stores canonical paths) | S |
| C8 | Pango's CoreText font map (the macOS default) gives different metrics: Brick's grid text is **clipped at the top** ("0" reads as "U", "7" as "/") | brick (grid cells, row and column headers); every app's text layout differs from Linux | `shots/arm64-brick-digits-coretext-clipped.png` vs `PANGOCAIRO_BACKEND=fc` `shots/arm64-brick-digits-fontconfig.png`: clean | either fix the grid's text placement for CoreText metrics, or run Pango on fontconfig on macOS (see Fonts below) | S-M |
| C9 | Fonts the suite relies on aren't on a Mac (F37); embedded .docx fonts need fontconfig's font map (F5) | slate (.docx, Markdown's Liberation Sans), frame (theme fonts), brick (Calibri default look) | `fc-match Calibri` → Verdana; Slate's .docx body falls back to Times-like serif under CoreText (`shots/arm64-slate-docx-cairo.png`), to Verdana-like under fontconfig (`shots/arm64-slate-docx-pango-fontconfig.png`); line heights differ from Linux | bundle Liberation, Carlito, Caladea (OFL) with a `fonts.conf` and use the fontconfig backend on macOS: one layout engine on both OSes, Slate's embedded fonts and metric aliases work unchanged | M |
| C10 | GTK doesn't follow macOS dark mode | all four | System Events `set dark mode to true`: menu bar and Dock turn dark, Brick/Slate/Etch stay light (`shots/arm64-dark-brick-recovery-alert.png`, `-dark-etch.png`); Brick's portal-based theme (F32) has no portal here | read `NSApp.effectiveAppearance` (or `AppleInterfaceStyle`) at start and on change, set `gtk-application-prefer-dark-theme`/the colour scheme | S-M |
| C11 | Menus: only Etch builds a menu bar model (`set_menubar`). Brick, Slate, Frame get GTK's default macOS menus: app menu (About, Settings… greyed, Services, Hide ⌘H, Hide Others ⌥⌘H, Show All, Quit ⌃Q) + Edit + Window; their commands live in in-window header-bar menus. Their windows draw GNOME-style CSD (−, □, × on the right), while Etch gets native traffic lights | brick, slate, frame | System Events menu lists: Slate `Apple, slate, Edit, Window`; Etch `Apple, etch, File, Edit, View, Image` with Etch's own items (`shots/arm64-slate-app-menu.png`, `-etch-app-menu.png`, `-etch-file-menu-global.png`) | `set_menubar` with the existing menu models; on macOS prefer native title bars (no `HeaderBar` as titlebar, or `decoration-layout` with buttons on the left) | M |
| C12 | Pasting text another app put on the pasteboard does nothing; copying out works | slate (paste path `read_text_future`); others untested | Slate Ctrl+A, Ctrl+C → `pbpaste` = "Clipboard on macOS" ✓; Brick Ctrl+C → `pbpaste` = "2" ✓. `pbcopy` text then Ctrl+V into an empty Slate document: nothing, also after switching to Finder and back (`shots/arm64-slate-external-paste-empty.png`); Ctrl+V with Slate's own copy pastes it | check on a real Mac first; if it reproduces, it's in GTK's `gdkmacosclipboard` reading `public.utf8-plain-text` [INFERENCE] | M (if real) |
| C13 | Fast synthetic key bursts garble typing in GTK windows | all GTK text input | System Events `keystroke "hello world"` with no delay: Slate shows "helllwwwwd", a Brick cell "heeeeewwwww" (`shots/arm64-slate-typed-fast.png`, `-brick-typed-cell-crop.png`); TextEdit gets "Hello world" from the same burst (`shots/arm64-control-textedit-typed.png`). One key every 0.2 s types correctly, as key codes, characters, or `cliclick t:` (`shots/arm64-slate-typed-keycodes-spaced.png`, `-chars-spaced.png`). Once, keys sent right after launch reached nothing until the page was clicked | likely a GTK macOS event-translation issue under bursts [INFERENCE]; verify fast real typing and an input method (Japanese/Pinyin) on a real Mac | S to verify |

Not failures, but macOS-specific behaviour seen:
- Benchmarks run and print JSON on the runner (`logs/run2-arm64-bench-*.txt`), but the numbers
  are meaningless there: the GL renderer is software and black, and Brick reports the
  virtual display's refresh as `0.1` Hz; Slate's time-sliced layout of a 143-word .docx took
  19.2 s (`open_ms`) because it measures per frame. Re-measure on real hardware.
- Commands launched from a terminal show as lower-case `brick`/`slate` with a generic `exec`
  Dock icon; inside an `.app` the menu bar shows `CFBundleName` (`Slate`, `Brick`).
- Brick's default window (1400×900) is wider than the 1024×768 runner display; macOS
  clamps it to 1195×674 at x=0, partly off screen.
- `GDK_SCALE=2` has no effect on macOS (scale comes from the screen); Retina (scale 2) could
  not be tried: both runners' displays are scale 1.

## What runs

With the C1 patch and `GSK_RENDERER=cairo`, every app starts, opens its files, and draws
them correctly on macOS 26 arm64:

- **Brick**: 20,000-row CSV with inferred column types, `00123`-style zips kept, status
  "20,000 rows" (`shots/arm64-brick-csv-cairo.png`); the XLSX fixture with its four sheets
  and `#DIV/0!` (`shots/arm64-brick-xlsx-cairo.png`). Crash recovery works: killing Brick
  with an unsaved edit and starting it again asks "Restore unsaved edits?" (journal + `File::lock`
  owner lock on APFS; `shots/arm64-dark-brick-recovery-alert.png`). Ctrl+C on a cell puts
  its value on the macOS pasteboard (`pbpaste` = `2`).
- **Slate**: Markdown (`shots/arm64-slate-md-cairo.png`), the .docx fixture with lists,
  picture, and table (`shots/arm64-slate-docx-cairo.png`), the start window
  (`shots/arm64-slate-start-cairo.png`). Typing, Ctrl+Z, and Ctrl+S work: after typing
  " hello world" and Ctrl+S the file on disk reads `Typed on macOS: hello world` (atomic save
  on APFS; `std`'s `sync_all` is already `F_FULLFSYNC` on Apple, so invariant 6 holds).
  Ctrl+O opens the **native NSOpenPanel** with Slate's filter "Documents (.txt, .md, .do…)".
- **Frame**: the fixture deck, slide list with thumbnails, inspector
  (`shots/arm64-frame-pptx-cairo.png`); a new blank deck with the caret in the title
  (`shots/arm64-frame-new-cairo.png`).
- **Etch**: a PNG, and a 1024×768 screenshot at Fit (`shots/arm64-etch-png-cairo.png`);
  its menu bar model becomes the **global macOS menu bar** with its items and keys, and its
  window gets native traffic lights.
- csv-engine's SIGBUS guard (F2) works on macOS: `truncation_while_mapped_reports_changed_instead_of_crashing`
  passes.
- `suite_file::recovery` (owner lock via `File::lock`/`try_lock` = `flock`), atomic save, and
  every other suite-file test except C7 pass on APFS.
- A self-contained **Brick.app** runs with Homebrew hidden (`sudo mv /opt/homebrew …`):
  dyld loaded 41 libraries from `Contents/Frameworks` and none from `/opt/homebrew`
  (`shots/arm64-brick-app-without-homebrew.png`).

Not tested: drag and drop (no Finder-to-window drag could be scripted), input methods,
Retina, printing, VoiceOver.

## Packaging and signing

Measured (arm64, run 2-3):
- `dylibbundler -of -b -x Brick.app/Contents/MacOS/brick -d …/Frameworks -p @executable_path/../Frameworks/`:
  16 s, copies and rewrites **41 dylibs**; Brick.app = **31 MB**; `hdiutil … -format UDZO`
  DMG = **12.5 MB**. dylibbundler re-signs ad hoc (`codesign --verify --deep --strict` passes);
  arm64 refuses to load unsigned code, so any later edit must be re-signed.
- Etch's dylib closure: 41 dylibs, 27.4 MB; libgtk-4 alone is 10.2 MB, and Homebrew's cairo
  drags in libX11 (1 MB) and friends. Runtime data if bundled: Adwaita icons 22 MB, gdk-pixbuf
  loaders 1.4 MB, GSettings schemas 36 KB. Brick.app ran without any of them; the other apps
  weren't tried relocated [INFERENCE: GTK 4 decodes PNG/JPEG/TIFF itself and Etch uses the
  `image` crate, so loaders are likely unnecessary].
- **Homebrew bottles target the runner's macOS**: on `macos-latest` libgtk/glib/pango have
  `minos 26.0`, so a bundle built there runs only on macOS 26+. The Rust binary itself is
  `minos 11.0`.
- `spctl --assess --type execute` rejects the ad-hoc-signed Brick.app (exit 3, "rejected"):
  a downloaded (quarantined) app needs Developer ID signing and notarization, or users must
  allow it in System Settings → Privacy & Security (macOS 15+ removed the Control-click
  bypass [INFERENCE]).
  Files fetched by `curl` carry no `com.apple.quarantine` attribute, so a curl installer like
  today's could install an ad-hoc-signed `.app` into `~/Applications` without Gatekeeper
  asking [INFERENCE, untested]; a DMG from a browser can't.
- A minimal `Info.plist` (`ci/Slate.Info.plist`: `CFBundleDocumentTypes` with
  `public.plain-text`, `net.daringfireball.markdown`,
  `org.openxmlformats.wordprocessingml.document`) registers with `lsregister` and
  `open -a Slate.app FILE` launches the bundle, but the file is dropped (C3).

Linux-only pieces and their macOS counterparts (inventory F-numbers):

| Linux today | macOS needs |
|---|---|
| `sites/*/install.sh`: `~/.local/bin`, `.desktop`, hicolor icon, `mimeapps.list` default (F39, F40) | `.app` per app (Info.plist, `.icns` from the SVGs via `iconutil`, document types/UTIs: CSV `public.comma-separated-values-text`, TSV `public.tab-separated-values-text`, XLSX `org.openxmlformats.spreadsheetml.sheet`, DOCX `org.openxmlformats.wordprocessingml.document`, PPTX `org.openxmlformats.presentationml.presentation`, MD `net.daringfireball.markdown`, PNG/JPEG/BMP `public.png`/`public.jpeg`/`com.microsoft.bmp`) in a DMG. macOS has no silent "make default" like `mimeapps.list`; the user picks it in Finder (Get Info → Open with) [INFERENCE] |
| `scripts/ship.sh`: x86_64 tarball, one `latest.json` (F38) | an arm64 DMG per app; `latest.json` with per-OS entries |
| No signing (F41) | Apple Developer Program ($99/yr), Developer ID Application certificate, `codesign --options runtime --timestamp` on every dylib then the app (inside out), `xcrun notarytool submit --wait` with an App Store Connect API key, `xcrun stapler staple` the DMG. No entitlements expected (no JIT; all code signed by one team) [INFERENCE] |
| XDG folders (F16-F19): `~/.local/state`, `~/.cache`, `~/.config` | They work as is (XDG vars unset → `$HOME` fallbacks; Slate's fonts go to `~/.cache` without `XDG_RUNTIME_DIR`). Native would be `~/Library/Application Support/<bundle id>` and `~/Library/Caches/<bundle id>` |
| `/proc/<pid>` liveness (F13) | C5 |
| `/proc/self/status` RSS in Brick's bench modes (F21-F23) | returns 0 (`"rss_anon_kib":0` in the bench JSON); `task_info`/`proc_pid_rusage` if wanted |
| `posix_fadvise` in perf tests (F6) | C6 |
| fontconfig FFI in Slate (F5) | links and loads fine (Homebrew ships libfontconfig and libpangoft2); inert under CoreText, works under `PANGOCAIRO_BACKEND=fc` |
| Rename atomicity, fsync | APFS `rename` is atomic; `sync_all` = `F_FULLFSYNC` (stronger and slower than Linux `fsync`); dir fsync succeeds |
| `File::lock` | `flock`: works (recovery prompt seen) |
| Case-sensitive paths (F11) | APFS is case-insensitive by default: same file under two spellings can appear twice in Recent and "already open" checks [not exercised] |

Universal binaries: Homebrew installs one architecture, so a universal app means two GTK
builds merged dylib by dylib with `lipo` (about twice the 31 MB). Not worth it: Apple has
said macOS 26 is the last release for Intel Macs [INFERENCE from Apple's WWDC 2025
announcement]. Ship arm64; an Intel DMG from `macos-15-intel` (GTK 4.22.4, `minos` 15) is
possible if asked.

## Recommendation

**Toolchain.** Homebrew GTK 4 + stable Rust on GitHub's macOS arm64 runners, as here; no
cross-compiling from Linux (GTK must come from a Mac). Build releases on the oldest arm64
runner image that GitHub still offers (`macos-14` today) so the bundled Homebrew dylibs carry
that `minos` (Sonoma 14+) [INFERENCE: bottles are per macOS version; check `otool -l` minos on
that runner]. Building GTK ourselves with `MACOSX_DEPLOYMENT_TARGET` (gtk-osx/jhbuild) is the
fallback if an older floor is needed (L).

**Bundling.** A `scripts/bundle_macos.sh`: `.app` skeleton per app, `dylibbundler` (or the
same in 40 lines of `otool -L`/`install_name_tool`), bundled fonts + `fonts.conf` if C9 goes
the fontconfig way, `.icns`, Info.plist with document types, DMG via `hdiutil`. About 31 MB
per app, 12.5 MB per DMG. Each app bundles its own GTK (4 × ~30 MB); a single "Cronkite"
DMG with four apps doesn't shrink that. **M** (2-3 days).

**Signing.** Developer ID + hardened runtime + notarization in CI with secrets (p12 + password,
App Store Connect API key). **M** (2 days) + $99/yr + an Apple account the user owns.

**CI.** Add a `macos` job to `ci.yml` on `macos-latest`: `brew install gtk4 pkgconf`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (after C6
and C7). Measured on a cold cache (run 2): brew 35 s; check, all-targets check, and clippy
152 s; tests target by target 294 s; release builds of all four 228 s. The docker/sway GUI checks (`scripts/check_*.sh`) don't port; this
spike's `run.sh` shows a runner can start apps, drive them with System Events and
`cliclick`, and screenshot them, with `GSK_RENDERER=cairo`. **S** for the job, **L** for
real GUI checks.

**Product work for a usable macOS build**, in order:

| Work | Causes | Effort |
|---|---|---|
| Accept macOS in `suite-platform` | C1 | S |
| Verify the GL renderer on a real Mac; else force cairo on macOS | C2 | S |
| Fix the four test targets and three tests | C6, C7 | S |
| Slate's sweep liveness | C5 | S |
| `HANDLES_OPEN` + `connect_open` in all four apps | C3 | M |
| ⌘ shortcuts (`<Primary>`, one primary mask, labels from accelerators) | C4 | M |
| Global menu bar for Brick, Slate, Frame; native title bars | C11 | M |
| Fonts: fontconfig backend + bundled metric fonts (fixes Brick's clipping and Slate's .docx fidelity) | C8, C9 | M |
| Follow dark mode | C10 | S-M |
| Verify typing speed, input methods, external paste, drag and drop, Retina on hardware | C12, C13 | S (+M if C12 is real) |
| App support folders under `~/Library` (optional; XDG fallbacks work) | F16 | S |
| Bundle, DMG, signing, notarization, release script, `latest.json` | — | M + M |
| CI job | — | S |

Roughly 3 weeks for a signed arm64 DMG of each app that behaves like a Mac app; about 2
days for an unsigned, Linux-behaved build that opens files from the command line.

## Open questions

1. Does GTK's GL renderer draw on real Apple Silicon hardware (C2)? Every window was black
   on both runner VMs.
2. Is external paste (C12) a GTK bug on real hardware or a runner artefact? Does fast real
   typing and an input method work (C13)?
3. Minimum macOS: ship what `macos-14` bottles give (14+), or build GTK for an older target?
4. Fonts on macOS: keep CoreText (native look, fix Brick's metrics) or fontconfig with bundled
   Liberation/Carlito/Caladea (same layout as Linux, Slate's embedded fonts work)?
5. Native or XDG folders on macOS (F16)?
6. Who owns the Apple Developer account and certificates, and is $99/yr in scope?
7. Window chrome: keep GNOME-style header bars on macOS, or native title bars with the
   commands in the global menu bar (C11)?
