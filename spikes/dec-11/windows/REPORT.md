# DEC-11 spike: Windows x86_64

**Bottom line:** the product code doesn't build for Windows today. Two library crates use
Unix-only APIs, and everything depends on one of them. With 8 small experiments (34 lines,
listed below), all four apps build with both Windows toolchains, start on a Windows runner,
open their sample files, and draw them correctly, including from a self-contained folder
with nothing on `PATH`. After that, 7 test targets still don't compile (all Unix-only test
code). 9 tests fail, for four reasons: the recent-files list (4), mapped files that can't be
truncated (2), a timing-sensitive test (1), and CRLF fixtures (2, fixed by a CI setting). The
runtime problems that matter are:

- every save reports an error;
- Slate deletes other Slate windows' working copies;
- the state folders land in odd places;
- each app opens a console window;
- the default fonts are missing.

Recommendation: build natively on `windows-latest` with MSYS2 UCRT64, ship a per-user
installer of one shared GTK runtime plus four exes (about 32 MB zipped), and sign it.

Everything here was run and observed unless marked [INFERENCE].

## Environment

| | |
|---|---|
|Base commit|`6a26f18` (DEC-11 story added), branch `spike/dec-11-windows`|
|Windows runner|GitHub `windows-latest` = Windows Server 2025 Datacenter, build 26100; 1024×768; no GPU|
|MSYS2 job|`msys2/setup-msys2` UCRT64, MSYS2's own `mingw-w64-ucrt-x86_64-rust`: rustc 1.99.0, host `x86_64-pc-windows-gnu`; GTK 4.24.1, Pango 1.58.2, GLib 2.90.1. Setup takes 122 s.|
|MSVC job|`dtolnay/rust-toolchain@stable`: rustc 1.99.1, `x86_64-pc-windows-msvc`; GTK from gvsbuild's 2026.8.0 release zip (`GTK4_Gvsbuild_2026.8.0_x64.zip`: 301 MB, 1.17 GB unpacked, 36 s to download and unpack; includes `pkgconf`, `fontconfig.lib`, `pangoft2-1.0.lib`): GTK 4.22.4, Pango 1.58.0, GLib 2.88.3|
|Cross build (local)|docker `fedora:latest` (Fedora 44): `mingw64-gtk4` 4.21.0, `mingw64-pango` 1.57.0, `mingw64-glib2` 2.88.3, MinGW GCC 16.2.1; rustup stable 1.99.1, target `x86_64-pc-windows-gnu`; Wine 11.0 (Staging) on Xvfb. The machine has 24 cores. Image: `cross.Dockerfile`, 6.2 GB.|
|Cross build (CI)|the same packages in a `fedora:latest` container on `ubuntu-latest`, then a second job runs the bundle on `windows-latest`|

### CI runs

Workflow: `.github/workflows/spike-windows.yml`; scripts and probes are in this folder.

1. Baseline, product code unchanged: <https://github.com/avdrav1/cronkite/actions/runs/38781634209>
2. With the compile experiments and the platform gate opened: <https://github.com/avdrav1/cronkite/actions/runs/38782356590>
3. Adds the directory-fsync experiment, turns off `core.autocrlf`, and falls back to running test targets one by one: <https://github.com/avdrav1/cronkite/actions/runs/38784682656>
4. Adds the cross-build jobs: <https://github.com/avdrav1/cronkite/actions/runs/38784842662>
5. Adds window captures, a `gdbus` count, and a corrected truncation probe: <https://github.com/avdrav1/cronkite/actions/runs/38787644715>
6. Cross jobs only, plus the cairo renderer: <https://github.com/avdrav1/cronkite/actions/runs/38789516108>

The downloaded artifacts are in `ci/run1` … `ci/run6`; local cross-build logs are in `cross-logs/`.

### Experiments on this branch

Each is a finding, not a fix. They're all marked `DEC-11 SPIKE EXPERIMENT`.

|File|Change|Why|
|---|---|---|
|`crates/suite-file/src/recent.rs`|`as_bytes` → `as_encoded_bytes`; `OsStr::from_bytes` → lossy UTF-8|C1 (from the inventory slice's `experiments.patch`)|
|`crates/suite-file/src/lib.rs`|test-only `OsStrExt` → `cfg(unix)`|C1 (same patch)|
|`apps/brick/crates/csv-engine/src/source.rs`|SIGBUS guard behind `cfg(unix)`, no-op stub elsewhere|C2 (same patch)|
|`apps/brick/crates/data-model/src/journal.rs`|path bytes through `as_encoded_bytes` / lossy UTF-8|C3 (same patch)|
|`apps/etch/src/recovery.rs`|the same for Etch's journal|C4|
|`apps/slate/src/fonts.rs`|drop mode `0700`; `as_encoded_bytes` for the fontconfig path|C5|
|`crates/suite-platform/src/lib.rs`|`"linux" \| "windows" => Ok(Platform::Linux)`|R1, to see the apps run|
|`crates/suite-file/src/save.rs`|`#[cfg(unix)]` on `File::open(dir)?.sync_all()?`|R2, to see the failures after it|

## Build matrix

Native Windows, MSYS2 and MSVC alike (the two toolchains gave identical results unless a cell
says otherwise). "Baseline" is run 1. "With experiments" is run 3 (tests) and run 2/3
(check and build). Test counts are passed / failed / ignored.

### Crates

|Crate|check, baseline|check, with experiments|tests, with experiments|
|---|---|---|---|
|suite-core|ok|ok|1/0/0|
|suite-ui|ok|ok|1/0/0|
|suite-platform|ok|ok|1/0/0|
|suite-file|**fails (C1)**|lib ok; test `atomic` fails (C6)|5/**4**/0: `recent::tests::*` (R3); `atomic` doesn't compile|
|csv-engine|**fails (C2)**|lib ok; tests `fetch_perf`, `index_perf` fail (C6)|28/**2**/1: `source::tests::truncation_while_mapped_…`, `index::tests::row_spans_report_a_file_changed_underneath` (R4)|
|formula|ok|ok|41/0/0|
|data-model|blocked by C2|ok|106/0/7. Without the `autocrlf` fix it's 104/**2** (R9).|
|grid|blocked|ok|25/0/0|
|commands|blocked|ok|60/0/11|
|file-format|blocked|ok|10/0/5 on MSVC. MSYS2: 9/**1**/5, `kill_9_mid_save_leaves_the_original_intact` ("the child finished or never started writing before it could be killed": timing). Before the R2 experiment: 3/**7**/5.|
|jobs|blocked|ok|3/0/6|
|xlsx|blocked|ok|24/0/5 (before R2: 17/**7**/5)|
|brick (bin)|blocked|ok; test `first_rows` and bench `corpus` fail (C6)|25/0/7 (unit tests); `first_rows` doesn't compile|
|document-model|ok|ok|38/0/0|
|docx|blocked by C1|ok|54/0/7 (before R2: 35/**19**/7)|
|flow|ok|ok|32/0/0|
|document-edit|blocked (dev-deps)|ok|58/0/0|
|slate (bin)|blocked (C1, C5)|ok|20/0/5|
|presentation-model|ok|ok|1/0/0|
|pptx|blocked by C1|ok|44/0/5 (before R2: 36/**8**/5)|
|deck-edit|blocked (dev-deps)|ok|38/0/0|
|frame (bin)|blocked|ok|0/0/1|
|image-model|ok|ok|85/0/7|
|image-io|blocked by C1|lib ok; test `save` fails (C6)|13/0/0; `save` doesn't compile|
|etch (bin)|blocked (C1, C4)|bin ok; bin test fails (C6: `OsString::from_vec` in `recovery.rs:375`)|the unit tests don't compile|

Per-crate logs are in `ci/run3/{msys2,msvc}/logs/test-<crate>.log`, and the one-line summary
is in `summary.txt`. Under Wine (cross build, `cross-logs/wine-tests/`, before the R2
experiment), the same save tests fail with os error 5: file-format 7, xlsx 7, docx 19, pptx 8.
Wine also fails `nasty_corpus`, data-model's `well_formed_files_have_no_problems`, and
recent's `labels_show_the_folder_from_home`, none of which fail natively. It misses the CRLF
failures, because that checkout has LF.

### Apps

|App|release build|`--version`|starts, opens its sample|time to window, MSYS2 bundle / MSVC|
|---|---|---|---|---|
|brick|baseline: fails (C1, C2). With experiments: ok, 31 MB GNU (4.0 MB stripped), 4.7 MB MSVC|`brick 0.3.0`|`sample.csv` (2,000 rows), `fixture.xlsx` (4 sheets), and no file (start window)|0.56 s / 9.6 s on the first launch on a fresh runner, then 0.43–1.4 s|
|slate|baseline: fails (C1, C5). With experiments: ok, 20 MB GNU (3.3 MB stripped), 3.7 MB MSVC|`slate 0.3.0`|`README.md`, `fixture.docx`|0.44–0.54 s|
|frame|baseline: fails (C1). With experiments: ok, 13 MB GNU (2.3 MB stripped), 2.6 MB MSVC|`frame 0.1.0`|`fixture.pptx` (5 slides), and no file (built-in blank deck)|0.44–0.55 s|
|etch|baseline: fails (C1, C4). With experiments: ok, 10 MB GNU (1.7 MB stripped), 1.9 MB MSVC|`etch 0.2.1`|`sample.png` (640×400)|0.55 s|

- The release build of all four apps takes 77–257 s on a 4-core runner, depending on the cache.
- `cargo check --workspace --all-targets` takes 12–88 s.
- Each running app's working set is 59–84 MB.
- The release profile keeps `debug = 1`, so GNU exes carry DWARF; MSVC puts it in PDBs.

## Failures by cause

### Compile blockers (in the product)

|#|Cause|Crates affected|Evidence (first error, verbatim, trimmed)|Likely fix|Effort|
|---|---|---|---|---|---|
|C1|`suite-file` stores paths as raw bytes with `std::os::unix::ffi::OsStrExt` (`recent.rs:10,28,44`, and lib tests)|`suite-file`, so **every app and every crate that depends on it**: in run 1, only suite-core, suite-ui, suite-platform, formula, document-model, flow, presentation-model, and image-model compiled|`error[E0433]: cannot find unix in os --> crates\suite-file\src\recent.rs:10:14 \| use std::os::unix::ffi::OsStrExt;` and `no associated function or constant named from_bytes found for struct OsStr` (run 1 `check.log`)|`as_encoded_bytes` / `OsStr::from_encoded_bytes_unchecked` (sound for bytes this process wrote), or store UTF-8|S|
|C2|Brick's SIGBUS guard in `csv-engine/src/source.rs:73-182` uses `libc::sigaction`, `mmap MAP_FIXED`, `sysconf`|`csv-engine`, so all of Brick|21 errors, starting with `error[E0425]: cannot find type sigaction in crate libc --> apps\brick\crates\csv-engine\src\source.rs:99:46`|`#[cfg(unix)]`. Windows doesn't need it: it refuses to truncate a mapped file (R4).|S|
|C3|`data-model/src/journal.rs:32` `OsStrExt`/`OsStringExt` for the journal's path|Brick|found by `check` once C1 and C2 were stubbed (inventory slice, and the local cross check)|as C1|S|
|C4|`apps/etch/src/recovery.rs:24` `OsStrExt`/`OsStringExt` (and test `:375`)|Etch|grep, then the local cross check (`cross-logs/check-experiments.log`: `no associated function … from_vec … recovery.rs:375`)|as C1|S|
|C5|`apps/slate/src/fonts.rs:28` `DirBuilderExt`/`PermissionsExt` (`mode(0o700)`) and `:228` `OsStrExt`|Slate|grep. Fixed in an experiment before `check` reached it (C1 hid it).|`cfg(unix)` the mode bits|S|
|C6|Unix-only test and bench code: `suite-file/tests/atomic.rs` (`PermissionsExt`), `image-io/tests/save.rs:170-180`, `csv-engine/tests/{fetch,index}_perf.rs` and `apps/brick/tests/first_rows.rs` plus `benches/corpus.rs` (`std::os::fd`, `libc::posix_fadvise`), the `etch` bin test (`OsString::from_vec`)|tests only|`error[E0432]: unresolved import std::os::fd --> apps\brick\tests\first_rows.rs:11:14`; `no associated function or constant named from_mode found for struct Permissions --> apps\etch\crates\image-io\tests\save.rs:175:58` (run 3 `check.log`)|`#[cfg(unix)]` or `cfg(target_os = "linux")` on those tests; Windows variants of the unwritable-folder tests use the read-only attribute|S|

GTK itself was never a build problem. `gtk4-sys`, `pango`, and `glib` found GTK through
`pkg-config` on MSYS2, on gvsbuild (MSVC), and on Fedora MinGW cross. Slate's
`#[link(name = "fontconfig")]` and `pangoft2-1.0` linked on all three.

### Runtime and behaviour (seen on the Windows runner)

|#|Cause|Apps|Evidence|Likely fix|Effort|
|---|---|---|---|---|---|
|R1|`suite_platform::current()` accepts only `"linux"`, and Slate, Frame, and Etch exit at startup with "unsupported operating system". Brick has no such gate.|Slate, Frame, Etch|`crates/suite-platform/src/lib.rs:39-44`; let through by an experiment|add `Platform::Windows`|S|
|R2|**Every save reports failure after the file was already replaced.** `save_atomic` fsyncs the folder with `File::open(dir)?.sync_all()`, and opening a directory fails on Windows.|all four: CSV, XLSX, DOCX, PPTX, and images, all through `suite_file::save_atomic`|fsprobe: `open dir and sync_all (save_atomic's last step): Err Access is denied. (os error 5)`. Run 2: 41 save tests fail with `Io(Os { code: 5, kind: PermissionDenied, message: "Access is denied." })` (file-format 7, xlsx 7, docx 19, pptx 8). Run 3 with `#[cfg(unix)]` on that line: all pass.|`cfg(unix)` the directory fsync. On Windows, `MoveFileExW(MOVEFILE_REPLACE_EXISTING \| MOVEFILE_WRITE_THROUGH)` or `FlushFileBuffers` on the file is the durability step.|S|
|R3|The recent-files list is always empty: `Recent::load` keeps only lines starting with `/`, and `canonicalize` gives `\\?\C:\…` verbatim paths, which would also show in menus|Brick, Etch (and Slate/Frame once they have one)|4 `suite_file::recent::tests` fail: `left: [] right: ["C:\\Users\\RUNNER~1\\…\\b.csv", …]`. fsprobe: `canonicalize="\\\\?\\C:\\Users\\…\\recent.csv" starts with '/': false`.|keep lines that are `Path::is_absolute`; strip the verbatim prefix (`dunce`)|S|
|R4|A memory-mapped file can't be truncated or recreated on Windows. Brick maps the CSV it shows, so while it's open, another program can't `>` or `File::create` over it, and the SIGBUS case can't happen. Renaming over a mapped or open file **works** on Windows (std uses POSIX rename semantics), so Brick's own save over its mapped source (inventory F8) is fine.|Brick, plus any other program writing that CSV|csv-engine: `Os { code: 1224, … "The requested operation cannot be performed on a file with a user-mapped section open." }` (2 tests). fsprobe: `set_len(0) on memory-mapped file: Err … (os error 1224)`, `File::create (truncate) …: Err … 1224`, `write in place, same length: Ok`, `rename over memory-mapped file: Ok ()`, `mapping still reads the old bytes: true`, `remove memory-mapped file: Ok`.|`cfg` the two tests. Product: decide whether "another program can't overwrite a file Brick has open" is acceptable (Excel locks open files too) or Brick should map a private copy; an ADR 0002 note either way.|S (tests), M (if it maps a copy)|
|R5|**Slate deletes other running Slates' package copies.** `Original::sweep` treats a pid with no `/proc/<pid>` as dead, so the first window's next .docx save can't find its original package.|Slate|run 3/5 `summary.txt`: `slate sweep: first slate pid=5552 copies in C:\Users\runneradmin\AppData\Local\Temp\slate before second start: [original-5552-0.docx] after: []`, on every toolchain and bundle|use the recovery store's owner-lock liveness (`suite_file::recovery`, whose locks work on Windows: fsprobe `try_lock, other process: would-block`) or `OpenProcess`|S|
|R6|Per-user folders come from `XDG_*` and `HOME`, and neither exists for a GUI app on Windows. Brick's state (recent list, recovery journals) goes to a **relative** `.local\state\brick` in whatever the working directory is. Etch's recent list goes to `%USERPROFILE%\.local\state\etch\recent`. Caches go to `%TEMP%\{brick,frame,slate}`, or `%USERPROFILE%\.cache\…` when Git Bash sets `HOME`. Slate's settings use `glib::user_config_dir`, so `%LOCALAPPDATA%\slate`, which is fine.|all|run 5 `summary.txt`: `files under the apps' working directory …\cwd: \.local\state\brick\recent`; `exists: C:\Users\runneradmin\.local -> \state\etch\recent`. fsprobe: `Brick's state dir without XDG_STATE_HOME: ".local/state\\brick" (absolute: false)`, `env HOME=None`.|one `suite_platform::dirs` helper (GLib's `g_get_user_state_dir` gives `%LOCALAPPDATA%`; toolkit-free crates get the folder passed in) for the 9 call sites (inventory F16-F18)|M|
|R7|Each app opens a **console window** next to its own: the exes are console-subsystem binaries|all|`shots/desktop-console-window-behind-brick.png`: a Windows Terminal tab titled with the exe path behind Brick's start window, in every desktop shot|`#![windows_subsystem = "windows"]` in release builds, plus `AttachConsole(ATTACH_PARENT_PROCESS)` so `--version`, `--bench-*`, and logs still print from a terminal|S|
|R8|Default fonts are missing: Slate's Markdown default "Liberation Sans" and the docx fixture's fonts, and Frame's theme "Calibri Light"; Pango falls back to its own Sans. Slate's embedded docx fonts go through fontconfig, but Pango on Windows uses the Win32/DirectWrite font map, so they won't be used [INFERENCE: from `fonts.rs`, not exercised].|Slate, Frame|`*.err.txt`: `couldn't load font "Liberation Sans 11", falling back to "Sans 11", expect ugly output.`; `couldn't load font "Calibri Light, 30.874px", falling back to "Sans 30.874px"`. The screenshots still look right, using Segoe UI and Courier.|bundle Liberation/Carlito/Caladea and register them per process (`AddFontResourceExW(FR_PRIVATE)`), or map the families; Windows embedded fonts go the same way (inventory F5, F37)|M|
|R9|The repo has no `.gitattributes`, so Windows runners (`core.autocrlf=true`) check out text fixtures with CRLF|tests|run 2: `a_v1_journal_still_restores` `left: "id,name,qty\r\n1,green apple,3\r\n…" right: "id,name,qty\n1,…"` (data-model 2 tests). Gone in run 3 with `git config --global core.autocrlf false`.|`.gitattributes` (`* text=auto eol=lf`, with fixtures `-text`)|S|
|R10|`kill_9_mid_save_leaves_the_original_intact` relies on a save outlasting a kill. It failed once on MSYS2 and passed on MSVC.|test|run 3 msys2 `test-file-format.log`: "the child finished or never started writing before it could be killed"|make the child's save longer or gate it|S|
|R11|No GPU on the runner. GSK falls back to cairo (software) on both native toolchains, so the GPU path is untested.|all|`Not using GL: libEGL not available` (MSYS2 4.24.1), `Not using GL: No GL implementation is available` (gvsbuild 4.22.4), `Not using Vulkan as fallback: … VK_ERROR_INCOMPATIBLE_DRIVER`, `Using renderer 'GskCairoRenderer' for surface 'GdkWin32Toplevel'`|test on real hardware (open question)|?|
|R12|GTK 4.22.4 (gvsbuild) and 4.21.0 (Fedora MinGW) log `Gtk-CRITICAL **: gtk_native_get_surface: assertion 'GTK_IS_NATIVE (self)' failed` in Slate and Etch, and Etch logs `Allocating size to GtkBox … without calling gtk_widget_measure()`. MSYS2's GTK 4.24.1 logs neither.|Slate, Etch on older GTK|run 2 `run-msvc/*.err.txt`; run 6 `run-cross-cairo/*.err.txt`|a newer GTK; nothing to do in the apps|—|
|R13|GLib autolaunches a D-Bus session bus on Windows: one `gdbus.exe` is running after the apps start. Brick's theme follows the settings portal over that bus and finds nothing, which is harmless. `gdbus.exe` must be in the bundle.|all|run 5 `summary.txt`: `gdbus_processes=1` on every launch|bundle `gdbus.exe`; `cfg(linux)` the portal code (inventory F32)|S|
|R14|On a fresh runner, the first launch takes 5.7–17.5 s to show its window (Brick, the first app started); later ones take 0.43–2.2 s|first launch|run 5: `brick-sample.csv: running window_after_ms=5669` (MSYS2), `9625` (MSVC), `17532` (cross). The second Brick took 0.46–2.2 s. [INFERENCE: Defender scanning about 60 new DLLs on first load.]|measure on a desktop install|?|

Not exercised here, since no UI was driven on Windows: the file dialogs (native through
`gtk::FileDialog` [INFERENCE]), clipboard, drag and drop, the input method (GTK's IMM32
context), keyboard shortcuts, HiDPI and fractional scaling, and dark mode.

## What runs

All four apps start, open their file, and draw it on Windows Server 2025, with **MSYS2**
(from `target\release` and from the clean-`PATH` bundle) and with **MSVC + gvsbuild**. Each
had a window titled with the file after 0.4–1.4 s (the first launch is slower: R14). The
screenshots are PrintWindow captures of each window, 1024×768 runner screen:

- **Brick**:
  - `shots/msys2-bundle-brick-sample.csv.png`: the grid, column type badges, and the format bar.
  - `shots/msys2-bundle-brick-fixture.xlsx.png`: 4 sheet tabs, `00007` kept as text, `#DIV/0!`, "Excel workbook".
  - `shots/msys2-bundle-brick-nofile.png`: "Open a CSV file".
  - `shots/msvc-brick-sample.csv.png`.
- **Slate**:
  - `shots/msys2-bundle-slate-README.md.png`: headings, a code block, links, and lists.
  - `shots/msys2-bundle-slate-fixture.docx.png`: runs, alignment, lists, a link, and the picture.
  - `shots/msvc-slate-fixture.docx.png`.
- **Frame**:
  - `shots/msys2-bundle-frame-fixture.pptx.png`: the slide list with all 5 thumbnails (shapes, group, table, hidden slide), the canvas, and the inspector.
  - `shots/msys2-bundle-frame-nofile.png`: the blank deck.
  - `shots/msvc-frame-fixture.pptx.png`.
- **Etch**:
  - `shots/msys2-bundle-etch-sample.png.png`: the tools, the palette, the 640×400 image at 100%, and the menu bar.
  - `shots/msvc-etch-sample.png.png`.
- **Console**: `shots/desktop-console-window-behind-brick.png` shows the whole desktop with the console window from R7.

**Under Wine** (the cross build, Xvfb 1280×800), all four start and open their files
(`shots/wine-*.png`). The documents render correctly, but the **UI font drops glyphs**: "Item 1"
shows as "Item .", and "sample.csv" as "samp e.csv". The same happens with
`GSK_RENDERER=cairo` (`wine-cairo/`), so it's Wine's fonts, not GTK: the same bundle on
Windows doesn't do it. Wine also gets file semantics wrong: renaming over an open or mapped
file gives `Access denied` under Wine and works on Windows (`wine/fsprobe-wine.txt` vs
`ci/run5/msvc-logs/probe/fsprobe.txt`). Wine is a smoke test, not a test platform.

**The cross-built bundle on Windows (run 5)**: all eight launches got a window with the right
title, but the **windows are black**. Fedora's MinGW GTK 4.21.0 picks the GL renderer through
EGL and can't make a surface: `Using renderer 'GskGLRenderer' for surface 'GdkWin32Toplevel'`,
then `Gdk-CRITICAL **: gdk_gl_context_ensure_egl_surface: assertion 'priv->egl_native_window != NULL' failed`
(`shots/cross-gl-slate-fixture.docx-black.png`). The run-6 retry with `GSK_RENDERER=cairo` is
below.

**With `GSK_RENDERER=cairo` (run 6)**, the same cross-built bundle draws everything
correctly. All eight launches showed their window in 0.43–0.46 s
(`shots/cross-cairo-frame-fixture.pptx.png`, `shots/cross-cairo-brick-sample.csv.png`), and
the captures match the native ones. So the black windows come from Fedora's GTK GL/EGL
build on a GPU-less machine, not from the cross build itself. On real GPUs it may differ
(open question 1).

## Cross-build verdict

Linux → Windows with Fedora's MinGW packages **builds** everything and **is fast**:

- 67 s for the 4-app release build on 24 cores, 86 s on a 4-core `ubuntu-latest` runner, against 77–257 s natively.
- The image is a single `dnf install`.
- The bundle script is 40 lines of `objdump -p`.
- The result is 50 DLLs, 104 MB with exes stripped, and 37 MB as a zip.

It is **not a good release path**:

1. **The GTK version is whatever Fedora packages.** It's 4.21.0 in Fedora 44, an odd-numbered *development* snapshot, behind MSYS2's 4.24.1. On the runner it chose GL through EGL and drew **black windows** unless `GSK_RENDERER=cairo` was set (above). Getting the right GTK means waiting for Fedora or patching its MinGW packages.
2. **Tests can't run natively.** Under Wine they run (`cross-logs/wine-tests/`), but Wine's file semantics and fonts differ from Windows (above), so a Windows runner is needed anyway.
3. **It's the same GNU ABI as MSYS2**, with no PDBs and no MSVC tooling, so it offers nothing MSYS2 doesn't.

It is useful for a quick local "does it still compile for Windows" check:
`cargo check --target x86_64-pc-windows-gnu` with `mingw64-gtk4` takes 5–29 s here.

## Toolchain recommendation

**MSYS2 UCRT64 on `windows-latest`, with MSYS2's own Rust (`x86_64-pc-windows-gnu`).**

- The `setup-msys2` step installs `gtk4`, `pkgconf`, `gcc`, `rust`, and `zip` in 122 s.
- It has the newest GTK (4.24.1) and the cleanest logs (no criticals).
- `ldd` makes bundling trivial (`bundle.sh`).
- Tests and the release come from the same toolchain.
- Both toolchains worked on the first CI attempt.

**MSVC + gvsbuild** is the second choice:

- It builds the same, gives smaller exes with PDBs and the MSVC debugger, and doesn't need MSYS2.
- It means a 301 MB zip pinned to gvsbuild's release cadence, with an older GTK (4.22.4, with the criticals in R12).
- There's no `ldd`, so a bundle needs `dumpbin /dependents` or similar (not built here).

Work to get there:

|Item|Effort|
|---|---|
|C1–C6 compile fixes (8 product files plus 7 test targets)|S|
|R1–R3, R5, R7, R9 (the platform gate, directory fsync, recent list, Slate sweep, windows subsystem, `.gitattributes`)|S each|
|R6 per-user folders helper (9 call sites)|M|
|R8 fonts (bundled metric-compatible fonts and private registration; embedded docx fonts)|M|
|R4 product decision on mapped files|S–M|
|CI: a `windows` job in `ci.yml` (this workflow's MSYS2 job is most of it), building, testing, and running the apps|S|

## Packaging and signing notes

**A self-contained folder works.** `bundle.sh` copies the exes, every `/ucrt64` DLL that
`ldd` reports for them, the GDK-Pixbuf loaders, and `gdbus.exe`, plus
`lib\gdk-pixbuf-2.0`, `share\glib-2.0\schemas\gschemas.compiled`, and
`share\icons\{Adwaita,hicolor}`, laid out as `bin\ lib\ share\`. The apps ran from it with
`PATH` set to only `C:\Windows\System32;C:\Windows` and no `XDG_*`. GLib finds `share\`
relative to its DLL, so no wrapper or environment variables are needed.

**Size (MSYS2, run 5):**

- 61 DLLs.
- 138 MB with unstripped exes; 77 MB with stripped exes (`bin` 66 MB, icons 7 MB, lib 5 MB).
- **32 MB `zip -9`**.
- The largest DLLs:
  - `libgtk-4-1.dll`: 12.3 MB.
  - `librsvg-2-2.dll`: 11.5 MB, needed for Adwaita's SVG icons through the pixbuf loader.
  - `libstdc++`: 2.6 MB.
  - HarfBuzz-subset: 2.0 MB.
  - GIO: 1.9 MB.
  - epoxy: 1.7 MB.
  - GStreamer: about 5 MB over 9 DLLs, pulled in by GTK's media backend; a custom GTK build without `media-gstreamer` would drop it [INFERENCE].
- The four stripped exes total 11.3 MB (Brick 4.0, Slate 3.3, Frame 2.3, Etch 1.7), so **one shared runtime for the suite** is about 66 MB of the 77, and per-app installers would each carry it.

**The cross bundle:** 50 DLLs, 104 MB stripped, 37 MB zip.

**Installer.** The simplest real path is a per-user installer, the counterpart of today's
`install.sh`:

- Inno Setup or WiX (`cargo-wix`) MSI.
- It installs to `%LOCALAPPDATA%\Programs\Cronkite`, with Start-menu shortcuts.
- File associations become ProgIDs under `HKCU\Software\Classes`, the counterpart of `mimeapps.list` and the `.desktop` `MimeType=` lines: `.csv .tsv .xlsx` → Brick, `.txt .md .docx` → Slate, `.pptx` → Frame, `.png .jpg .jpeg .bmp` → Etch. They also need "Open with" entries and icons as `.ico`, converted from `packaging/*.svg`.
- MSIX would work too, but it always needs a signature, and its virtualised `AppData` complicates R6.
- A portable zip (32 MB) works today, with R7 fixed.

Other packaging notes:

- **Release pipeline.** `scripts/ship.sh` and `latest.json` need per-platform artifacts (inventory F38). An `irm … | iex` PowerShell installer could mirror the curl one.
- **Signing.** Unsigned exes and installers get a SmartScreen "unrecognized app" warning. Authenticode signing means one of:
  - Azure Trusted Signing (cheap monthly subscription, needs identity validation);
  - an OV certificate (reputation builds slowly);
  - an EV certificate (immediate reputation, hardware token or cloud HSM).
- **Sign every PE.** `signtool` should sign the exes and, ideally, the bundled DLLs and the installer [INFERENCE: standard practice, not tried].
- **Defender's first-run scan** of about 60 unsigned DLLs may explain R14's 6–17 s first start [INFERENCE].

## Open questions

1. **GPU rendering on real hardware.**
   - The runner has no GPU, so only cairo ran natively.
   - MSYS2's GTK 4.24 says "libEGL not available": does it need ANGLE's `libEGL.dll`, or does it use WGL on a real GPU?
   - Does Vulkan work on typical drivers?
   - The cross-built GTK 4.21 chose EGL and drew nothing.
   - This needs a Windows desktop with a GPU, plus Brick's scroll and Etch's stroke benchmarks there.
2. **HiDPI** (125% and 150% scaling), dark mode (GTK 4.24 on Windows follows the system scheme? [INFERENCE]), and the native file dialog's filters, which are MIME-based (inventory F33).
3. **Fonts.** Ship Liberation, Carlito, and Caladea, or depend on Office's Calibri and Cambria? This decides Slate and Frame layout fidelity against Linux.
4. **Mapped files (R4).** Is "Brick holds the CSV so others can't overwrite it" acceptable on Windows?
5. **One suite installer** (one GTK runtime, about 66 MB shared) or four per-app ones (about 70 MB each)?
6. **Platform floor.** Minimum Windows version (MSYS2 UCRT needs Windows 10+) and Windows on ARM64 (MSYS2 has a CLANGARM64 environment; not tried).
7. **Signing provider and budget** (Azure Trusted Signing or an OV/EV certificate).
