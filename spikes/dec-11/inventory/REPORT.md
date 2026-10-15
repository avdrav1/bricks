# DEC-11 spike: inventory (static Linux-specific inventory and target check)

## Environment

- Host: Arch Linux x86_64 (kernel 7.2.8), system Rust 1.99.0 (Arch package, no rustup). For the
  cross targets, an isolated rustup was installed in `/tmp/dec11-rustup` + `/tmp/dec11-cargo`
  (stable 1.99.1, as `rust-toolchain.toml` asks for `stable`), with targets
  `x86_64-pc-windows-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`. Nothing was
  installed in `~`.
- `check.sh` (throwaway) runs `cargo check --all-targets --target T -p CRATE` for each
  toolkit-free crate (21 crates × 3 targets = 63 checks); target dir `/tmp/dec11-target`.
  A full warm run takes about 24 s (the first, cold one took 36 s).
- Results: `matrix-baseline.txt` and `logs-baseline/` (branch code as is),
  `matrix-experiments.txt` and `logs-experiments/` (with `experiments.patch`, three local
  changes that show what fails next; see below). Logs are each check's full output.
- The workspace has **no C dependencies below the GTK layer**: `cargo tree -i cc` finds no
  `cc` crate, zip/image/quick-xml/calamine/rust_xlsxwriter are pure Rust here. So the
  toolkit-free crates need no cross C toolchain or SDK to check. The GTK apps can't be
  cross-checked from Linux: `cargo check --target aarch64-apple-darwin -p brick` stops at
  `glib-sys v0.22.9: pkg-config has not been configured to support cross-compilation`
  (same for windows-gnu: glib-sys, gobject-sys, gio-sys, cairo-sys-rs build scripts). Those
  need the native runners (other slices).
- Gotcha recorded for the orchestrator: this session's first edits and logs landed in
  `/home/av/Projects/cronkite` (the tools resolve relative paths against that checkout). They
  were moved to this worktree and reverted there with `git apply -R` of exactly that diff;
  that checkout's `git status` is clean again.

## Target check matrix (toolkit-free crates)

`cargo check --all-targets` (lib, tests, benches, examples). "ok" = compiles; "FAIL(lib)"
= the library itself doesn't compile; "FAIL(dep)" = only because a dependency's lib fails;
"FAIL(test)" = lib compiles, a test target doesn't.

| Crate | win-gnu | win-msvc | mac arm64 | win (+exp) | mac (+exp) |
|---|---|---|---|---|---|
| suite-core | ok | ok | ok | ok | ok |
| suite-file | FAIL(lib) | FAIL(lib) | ok | FAIL(test) | ok |
| suite-platform | ok | ok | ok | ok | ok |
| suite-ui | ok | ok | ok | ok | ok |
| csv-engine | FAIL(lib) | FAIL(lib) | FAIL(test) | FAIL(test) | FAIL(test) |
| data-model | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| formula | ok | ok | ok | ok | ok |
| xlsx | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| grid | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| commands | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| file-format | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| jobs | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| document-model | ok | ok | ok | ok | ok |
| docx | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| document-edit | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| flow | ok | ok | ok | ok | ok |
| presentation-model | ok | ok | ok | ok | ok |
| pptx | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| deck-edit | FAIL(dep) | FAIL(dep) | ok | ok | ok |
| image-model | ok | ok | ok | ok | ok |
| image-io | FAIL(dep) | FAIL(dep) | ok | FAIL(test) | ok |

gnu and msvc behave identically (no C, no linking in `check`). Baseline: 36/63 ok. With the
experiments: every library compiles for all three targets; what remains is four Linux-only
test files.

**macOS** needs nothing for the libraries. Only `csv-engine`'s perf tests fail:

```
error[E0425]: cannot find function `posix_fadvise` in crate `libc`
  --> apps/brick/crates/csv-engine/tests/index_perf.rs:39:15
  --> apps/brick/crates/csv-engine/tests/fetch_perf.rs:37:15
```

**Windows**, in the order they show up (each one hides the next):

1. `suite-file` (lib), the root of most "FAIL(dep)" rows (xlsx, docx, pptx, file-format,
   jobs, image-io all use it):
   ```
   error[E0433]: cannot find `unix` in `os`
     --> crates/suite-file/src/recent.rs:10:14   use std::os::unix::ffi::OsStrExt;
   error[E0599]: no associated function or constant named `from_bytes` found for struct `OsStr`
     --> crates/suite-file/src/recent.rs:28:51
   error[E0599]: no method named `as_bytes` found for reference `&OsStr`
     --> crates/suite-file/src/recent.rs:44:52, :78:41
   ```
   (`lib.rs:31,47` is the same thing in a unit test.)
2. `csv-engine` (lib), 21 errors, all in the SIGBUS guard of `src/source.rs:99-153`:
   ```
   error[E0425]: cannot find type `sigaction` in crate `libc`  --> src/source.rs:99:46
   error[E0425]: cannot find function `mmap` in crate `libc`   --> src/source.rs:118:27
   ```
3. `data-model` (lib), only visible once 1 and 2 compile:
   ```
   error[E0433]: cannot find `unix` in `os`  --> apps/brick/crates/data-model/src/journal.rs:32:14
   error[E0599]: no method named `as_bytes` found for reference `&OsStr`  --> journal.rs:377:48
   error[E0599]: no associated function named `from_vec` found for struct `OsString`  --> journal.rs:418:64
   ```
4. Tests only: `suite-file/tests/atomic.rs:4,29,46` and `image-io/tests/save.rs:170-180`
   (`std::os::unix::fs::PermissionsExt`, `Permissions::from_mode`, to make a folder
   unwritable); `csv-engine/tests/{fetch_perf,index_perf}.rs` (`std::os::fd`, `madvise`,
   `posix_fadvise`).

`experiments.patch` (applied on this branch, as findings, not fixes): `#[cfg(unix)]` on the
SIGBUS guard plus a no-op `cfg(not(unix))` guard; `recent.rs` and `journal.rs` use
`OsStr::as_encoded_bytes` to write and a lossy UTF-8 decode to read; the unix-only
`lib.rs` test is gated. The lossy read is wrong for non-UTF-8 Unix paths; the real fix is
in the findings (F1, F3).

## Findings by area

Effort: S = under a day, M = a few days, L = a week or more. "Owner" says where the fix
belongs; CLAUDE.md gives OS integration to `suite-platform`, which today is only a
`Platform::Linux` enum and an `Unsupported` error (`crates/suite-platform/src/lib.rs:11-43`).

### Compile blockers (Windows)

| # | Where | What it does | Breaks | Fix | Owner | Effort |
|---|---|---|---|---|---|---|
| F1 | `crates/suite-file/src/recent.rs:10,28,44,78` | Recent list stores paths as raw bytes, one per line (`OsStrExt`) | Windows: doesn't compile | Write `as_encoded_bytes()`; read with `OsStr::from_encoded_bytes_unchecked` only for bytes this process wrote, or store UTF-8 and skip non-UTF-8 paths (Brick's menu already skips them, `apps/brick/src/main.rs:456`) | suite-file | S |
| F2 | `apps/brick/crates/csv-engine/src/source.rs:73-182` | Process-wide SIGBUS handler that maps a zero page over truncated mapped pages (`libc::sigaction`, `mmap MAP_FIXED`) | Windows: doesn't compile. Not needed there: Windows refuses to truncate a file with a mapped view (`ERROR_USER_MAPPED_FILE`) [INFERENCE]. macOS: compiles; whether macOS raises SIGBUS with `si_addr` the same way must be tested by running `source::tests::truncation_while_mapped_*` on a Mac | `#[cfg(unix)]` guard + no-op guard elsewhere (as in the patch) | csv-engine | S |
| F3 | `apps/brick/crates/data-model/src/journal.rs:32,377,418` | Recovery journal stores the document path as raw bytes | Windows: doesn't compile | Same as F1. Journals don't move between machines, so `as_encoded_bytes` / `from_encoded_bytes_unchecked` round-trip is sound on both | data-model | S |
| F4 | `apps/etch/src/recovery.rs:24` (+ tests 375-376) | Same as F3 for Etch's journal | Windows: Etch doesn't compile [from grep; GTK app not checkable here] | Same as F3 | etch | S |
| F5 | `apps/slate/src/fonts.rs:28,81-95,228,241,280` | Embedded fonts: writes faces to `$XDG_RUNTIME_DIR/slate/fonts` with mode 0700 (`DirBuilderExt`, `PermissionsExt`), then adds them to fontconfig over FFI (`#[link(name = "fontconfig")]`, `FcConfigAppFontAddFile`), path via `OsStrExt` | Windows: doesn't compile. macOS and Windows: Pango's font map there is CoreText / Win32-DirectWrite, not fontconfig, so `FcMap::of` returns `None` and embedded fonts aren't shown (logged warning, no crash); the `fontconfig` link must still resolve (Homebrew and MSYS2 both ship it [INFERENCE]) | cfg-gate the fontconfig module and mode bits to Linux; on Windows `AddFontResourceEx(FR_PRIVATE)`, on macOS `CTFontManagerRegisterFontsForURL(.process)`, or Pango ≥1.56's `pango_font_map_add_font_file` where the backend supports it | suite-platform (font registration is OS integration; Frame will need it too) | M |
| F6 | tests: `suite-file/tests/atomic.rs:4,29,46`, `image-io/tests/save.rs:170-180`, `csv-engine/tests/{fetch,index}_perf.rs`, `apps/brick/tests/first_rows.rs:11,25`, `apps/brick/benches/corpus.rs:23,62` | Make a dir unwritable with mode bits; drop page cache with `posix_fadvise`/`madvise` | Windows: don't compile. macOS: the `posix_fadvise` ones don't compile | `#[cfg(target_os = "linux")]` on these tests (the perf ones are Linux-reference-box tests anyway); for unwritable-dir tests on Windows use a read-only file attribute or skip | tests | S |

### Saving and file semantics

| # | Where | What | Breaks | Fix | Owner | Effort |
|---|---|---|---|---|---|---|
| F7 | `crates/suite-file/src/save.rs:105-109` | After the rename, fsyncs the directory: `File::open(dir)?.sync_all()` | Windows: `File::open` on a directory fails (`Access is denied`, os error 5; it needs `FILE_FLAG_BACKUP_SEMANTICS`) [INFERENCE from std's Windows open flags], so **every save in every app reports failure after the file was already replaced** | `#[cfg(unix)]` the directory fsync; on Windows `MoveFileExW(MOVEFILE_REPLACE_EXISTING \| MOVEFILE_WRITE_THROUGH)` gives the durability. macOS: `fsync` on a dir works but isn't a disk flush; `F_FULLFSYNC` is the macOS equivalent if invariant 6 wants it | suite-file | S |
| F8 | `save.rs:103` (`std::fs::rename(tmp, dest)`) with Brick's mapped source (`csv-engine/src/source.rs:37`, ADR 0002; the module comment at :11 relies on "rename leaves an open mapping valid") | Brick saves over the file it has memory-mapped | Windows: replacing a file that has a mapped view fails (`ERROR_USER_MAPPED_FILE` / access denied) [INFERENCE; must be tried on a Windows runner]. That's Brick's main save path (Ctrl+S on an opened CSV), and XLSX scratch copies are mapped too | Before replacing, copy the still-needed bytes or map a private temp copy (as non-UTF-8 files already are, `csv-engine/src/encoding.rs:120-135`), or drop the mapping and remap the saved file; needs an ADR 0002 note | csv-engine + file-format | M-L |
| F9 | `crates/suite-file/src/recovery.rs:43-44,101-102` | Owner lock with `File::lock`/`try_lock` (std ≥1.89) | Works on Windows (`LockFileEx`) and macOS (`flock`) [INFERENCE]; on Windows the lock is mandatory, so nothing else may read the lock file, which nothing does | none expected; verify `tests` on runners | suite-file | — |
| F10 | `crates/suite-file/src/recent.rs:27` | Reading the list keeps lines starting with `/` only | Windows: every entry is `C:\…`, so **the recent list is always empty** | Keep lines that parse as absolute paths (`Path::is_absolute`) | suite-file | S |
| F11 | `recent.rs:43` (`canonicalize`) | Makes paths absolute and resolves symlinks | Windows: returns verbatim `\\?\C:\…` paths, which then show in menus/labels and won't match a path opened via the dialog, so the same file appears twice; macOS: case-insensitive APFS means `a.csv` and `A.csv` are one file but two entries (also Slate's/Etch's "already open → focus that window" lookups compare paths byte-for-byte) | `dunce::canonicalize`-style stripping on Windows; compare by file identity (dev+inode / file index; `same-file` crate) for "already open" | suite-file / suite-platform | S |
| F12 | `recent.rs:86-100` (`label`) | Shows the folder with `$HOME` as `~`, joined with `/` | Windows: `HOME` unset (it's `USERPROFILE`), so no `~`; if it were, `format!("~/{}")` mixes separators | Home from `glib::home_dir()` or `std::env::home_dir()` (un-deprecated in 1.86); use `Path::join` + display | suite-file | S |
| F13 | `apps/slate/src/saving.rs:96-110` (`Original::sweep`, spawned at `apps/slate/src/main.rs:159`) | Deletes package copies (`$XDG_CACHE_HOME/slate/*`) whose `pid` has no `/proc/{pid}` | macOS and Windows: no `/proc`, so **every running Slate's copies look dead and are deleted** at each launch; the next save of an open .docx then can't find its original package | Use the recovery store's owner-lock liveness (`suite_file::recovery`), or `sysinfo`/`kill(pid,0)`/`OpenProcess`. Frame's `pptx/src/save.rs:55-90` keeps copies the same way but has no sweep (killed Frames leak copies on every OS) | suite-file (process liveness) | S |
| F14 | line endings: `csv-engine/src/detect.rs:31-34`, `slate/crates/document-model/src/write/mod.rs:133-139`, `apps/slate/src/saving.rs:140` | Opened files keep their line ending (CRLF preserved); new files and new CSVs are LF | Not broken: Notepad has read LF since 2018. Product choice whether a new file on Windows should default to CRLF | Optional: `LineEnding::native()` default for new documents | document-model / csv-engine | S |
| F15 | `apps/brick/crates/xlsx/tests/import_export.rs:363-367` and extension checks | `.XLS` upper case is already handled (test) | macOS/Windows case-insensitive FS: no extension issues found. Recovery/journal/lock names are lower-case ASCII, fine | — | — | — |

### Per-user folders (XDG)

All of these follow XDG by hand. glib's `user_*_dir` already does the right thing per OS
(Windows: `%LOCALAPPDATA%`; macOS: glib keeps XDG-style `~/.config`, `~/.local/share`,
`~/.cache` [INFERENCE: glib docs]).

| # | Where | Variable / path | Breaks | Fix | Effort |
|---|---|---|---|---|---|
| F16 | `apps/brick/src/state.rs:15-23` | `$XDG_STATE_HOME` else `$HOME/.local/state`, `unwrap_or_default()` on `HOME` | Windows: no `HOME` → **relative path `.local/state/brick` in the working directory** (recent list and recovery journals land wherever Brick was started; from a Start-menu shortcut that may be `C:\Windows\System32` and unwritable) | One `suite_platform::dirs::{state,cache,config,runtime}(app)` using glib (state: `g_get_user_state_dir`, GLib ≥2.72) or the `directories` crate; macOS: decide `~/Library/Application Support/<id>` vs XDG | M (one helper, 9 call sites) |
| F17 | `apps/etch/src/recent.rs:21-29`, `apps/etch/src/recovery.rs:57-65`, `apps/slate/src/recovery.rs:51-59` | same, falling back to `glib::home_dir()` / `$HOME` / temp dir | Windows: falls back to `%TEMP%` (Slate/Etch recovery), which cleanup tools empty: recovery lost | same helper | (in F16) |
| F18 | `apps/brick/crates/csv-engine/src/encoding.rs:120-135`, `apps/frame/crates/pptx/src/save.rs:55-90`, `apps/slate/src/saving.rs:86-92` | scratch / package copies in `$XDG_CACHE_HOME` else `$HOME/.cache` else temp dir | Works (temp dir on Windows); engine crates can't use glib, so the cache dir must be passed in or come from a toolkit-free helper (`suite-platform` is toolkit-free today) | pass the dir in from the app, or `directories` crate in suite-platform | S |
| F19 | `apps/slate/src/fonts.rs:81-85` | `$XDG_RUNTIME_DIR` else `glib::user_cache_dir` | see F5 | — | (F5) |
| F20 | `apps/slate/src/settings.rs:122-125`, `apps/etch/src/settings.rs:113-116` | `glib::user_config_dir()` | Fine on both (glib picks the OS folder) | — | — |

### Processes, /proc, environment

| # | Where | What | Breaks | Fix | Effort |
|---|---|---|---|---|---|
| F21 | `apps/brick/src/main.rs:2740-2750` | Bench modes read `RssAnon`/`VmHWM` from `/proc/self/status` | Returns 0 elsewhere (`.ok()`), benchmarks only | `memory-stats` crate or cfg(linux) | S |
| F22 | tests: `commands/tests/{clear,columns,history}_1gb.rs:14-18`, `xlsx/tests/import_export.rs:487-491` | RSS asserts via `/proc/self/status` (`unwrap()`) | Panic on macOS/Windows; they're `#[ignore]` perf tests | cfg(linux) | S |
| F23 | `apps/brick/benches/corpus.rs:668-690` | `/proc/cpuinfo`, `/proc/meminfo` for the machine line | bench only | cfg or `sysinfo` | S |
| F24 | ~20 test files (`docx/tests/*`, `pptx/tests/*`, `xlsx/tests/*`, `data-model/tests/{malformed,paste_libreoffice}.rs`, `csv-engine/tests/nasty_corpus.rs`, `commands/tests/paste_from_libreoffice.rs`) | `Command::new("python3")` + LibreOffice UNO scripts | Windows: `python3` is usually `python`/`py`; LibreOffice's Python is separate. These are LibreOffice-comparison tests already gated on LO being present [INFERENCE: not each verified] | keep Linux-only in CI | S |
| F25 | `apps/brick/tests/*`, `apps/frame/tests/draw_perf.rs`, `apps/slate/tests/flow_perf.rs` | spawn the app binary with `--bench-*` (needs a display) | need a desktop session on the runner | Linux-only | — |
| F26 | `apps/*/src/main.rs` (`brick:76`, `frame:50` use `std::env::args()`) | argv as UTF-8 | panics on a non-Unicode argument (rare on Windows, possible on Linux); Slate/Etch use `args_os` already | `args_os` | S |

### GTK, input, shortcuts, menus, theme

| # | Where | What | Breaks | Fix | Owner | Effort |
|---|---|---|---|---|---|---|
| F27 | `Cargo.toml:74` `gtk4 0.11, features ["v4_14"]`; `apps/slate/Cargo.toml:29` `pango 0.22, ["v1_50"]` | minimum GTK 4.14, Pango 1.50 | Homebrew and MSYS2 both ship far newer GTK 4 (4.18-4.20 era) and Pango 1.5x [INFERENCE, not checked online here] | none; the Ubuntu 24.04 floor stays the binding one | — | — |
| F28 | `<Control>` accelerators: `apps/slate/src/main.rs` (25, e.g. 153-156, 211-278), `apps/etch/src/keys.rs:60-73` (table, 8), `apps/brick/src/main.rs:205-206`, `apps/frame/src/main.rs:106-107` | menu/app shortcuts | macOS: shortcuts are Ctrl+…, not ⌘. GTK maps `<Primary>` to ⌘ on macOS and Ctrl elsewhere | `<Control>` → `<Primary>` (mechanical) | apps | S |
| F29 | `CONTROL_MASK` in key handlers: `apps/brick/src/editor.rs` (8), `grid_view.rs:5283`, `main.rs:1028-1042`; `apps/etch/src/{canvas.rs:1668,keys.rs:43,text.rs}`; `apps/frame/src/{editing,outline,window}.rs`; `apps/slate/src/{find_bar,view}.rs` (22 uses) | hand-written shortcut matching | macOS: ⌘ arrives as `META_MASK`, so ⌘C/⌘V/⌘Z in the grid, canvas, and page don't work. Word-wise navigation is Alt (Option)+arrow on macOS, Ctrl on Linux/Windows | one `suite_ui::primary_mask()` (GTK has no public "primary modifier" API in gtk4-rs; `cfg(target_os = "macos")` → `META_MASK`) and route every handler through it; Etch's `keys.rs` table is the model to follow | suite-ui | M |
| F30 | shortcut labels: tooltips like `"Open… (Ctrl+O)"` (`apps/slate/src/main.rs:206`), `etch/src/keys.rs` `menu_item` | text says Ctrl | macOS shows "Ctrl" | derive labels from the accelerator (`gtk::accelerator_get_label`) | suite-ui | S |
| F31 | menus: Etch `app.set_menubar` + `show_menubar(true)` (`apps/etch/src/window.rs:136,400`); Brick/Frame/Slate: header bar + `MenuButton`/`PopoverMenu` (`brick/src/main.rs:446`, `slate/src/main.rs:203-246`) | in-window menus | macOS: Etch's menubar goes to the global menu bar (good). The others have no app menu bar; GTK supplies a minimal app menu (Quit) [INFERENCE]. Works, looks non-native. Windows: CSD header bar works | macOS: `set_menubar` for all four with the same models | apps | M |
| F32 | `apps/brick/src/theme.rs:1-100` | dark/light from the freedesktop settings **portal over the D-Bus session bus** (`gio::DBusProxy::for_bus_future(Session, …)`) | macOS/Windows: no portal; the code tolerates that ("Without a portal nothing changes"). On Windows GDBus may try to autolaunch a session bus [INFERENCE]. GTK 4.20+ follows the system scheme on macOS/Windows itself [INFERENCE] | cfg(linux) the portal path; elsewhere rely on GTK's `gtk-interface-color-scheme` | suite-platform | S |
| F33 | file dialogs: `gtk::FileDialog` everywhere (+ one `FileChooser` use, `apps/brick/src/main.rs`), filters by MIME (`brick/src/main.rs:485-506`) | portal dialog on Linux | GTK uses native dialogs on Windows/macOS (since 4.10) [INFERENCE]; MIME filters go through `g_content_type_from_mime_type`, which on Windows is extension-based: `text/csv` → `.csv` only if the registry knows it [INFERENCE] | add `add_suffix` patterns next to every MIME type | apps | S |
| F34 | file open from the OS: all four apps use `ApplicationFlags::NON_UNIQUE` and read files from argv (`brick/src/main.rs:76,179`, `etch/src/main.rs:58,113`, `frame/src/main.rs:50,96`, `slate/src/main.rs:69,106`) | double-click opens via `Exec=app %f` | Windows: file associations pass argv: works. macOS: Finder "Open With", double-click, and drop-on-Dock send an Apple Event, which GTK delivers as `GApplication::open`, needing `HANDLES_OPEN` + `connect_open` [INFERENCE: GTK macOS backend behaviour]; argv is empty, so **double-clicking a file starts an empty app** | `HANDLES_OPEN` + `connect_open` routed to the existing open path | apps | M |
| F35 | clipboard (`brick/src/editor.rs`, `slate/src/view.rs`), DnD, IME | GTK `gdk::Clipboard`, drop targets, `IMContext` | GTK covers macOS/Windows; Brick's paste-from-LibreOffice reads `text/html` + TSV, which on Windows arrive as "HTML Format" (GTK converts [INFERENCE]) | verify on runners | — | — |
| F36 | Wayland/X11 | no `gdk4-wayland`/`gdk4-x11` uses in product code; Wayland protocols only in `scripts/protocols/input-method-unstable-v2.xml` and `scripts/ime_helpers.py` (test harness) | nothing in the apps | — | — | — |
| F37 | `apps/slate/src/view/shaper.rs:7-11`, `slate/crates/document-model/src/fonts.rs:73-80` | relies on fontconfig metric aliases (Calibri → Carlito, Cambria → Caladea, SLATE-20) and generic families | macOS/Windows: no aliases. Windows with Office has Calibri; stock macOS has neither Calibri nor Carlito, so .docx text falls back by class and **line breaks differ from Linux** | bundle Carlito/Caladea/Liberation (OFL/SIL) and register them at startup (F5's mechanism), or map families in the app | suite-platform (fonts) | M |

### Packaging, install, release, CI

| # | Where | What | Breaks / what's missing | Effort |
|---|---|---|---|---|
| F38 | `scripts/ship.sh:26,59-130` | `TARGET=x86_64-unknown-linux-gnu`, builds in an Ubuntu 24.04 container, tarball `APP-V-x86_64-linux.tar.gz`, `sha256sum`, `stat -c%s`, one `latest.json` per app | Linux-only by design: `latest.json` has one `url`; per-platform artifacts need `latest.json` to grow per-OS entries (and the installers to pick one). Building macOS needs a Mac (no cross-compile of GTK apps from Linux, see Environment); Windows can cross-build with MSYS2 in CI | M |
| F39 | `sites/*/install.sh` (e.g. `sites/brick/install.sh:22-28,119-133`) | POSIX sh: `~/.local/bin`, `.desktop` + hicolor icon, `mimeapps.list`, GTK hint for apt/pacman/dnf | Linux-only. macOS: a `.dmg`/zip of a `.app` bundle (Info.plist with `CFBundleDocumentTypes` for CSV/XLSX/DOCX/PPTX/PNG, icons `.icns`), GTK + its dylibs bundled and relocated (Homebrew's GTK isn't relocatable: `gtk-mac-bundler`, or `dylibbundler` + GDK-Pixbuf loaders + GSettings schemas + icon theme). Windows: installer (MSI/WiX, Inno, or MSIX) bundling GTK's DLLs, `share/glib-2.0/schemas`, the Adwaita icon theme, and pixbuf loaders, plus registry file associations (ProgIDs) | L each |
| F40 | `packaging/dev.cronkite.*.desktop` | `Exec=app %f`, `MimeType=` lists | Linux-only; macOS maps to Info.plist, Windows to ProgIDs | (F39) |
| F41 | signing | none today (Linux tarball + sha256) | macOS: Developer ID Application cert ($99/yr Apple Developer Program), hardened runtime, `codesign` every bundled dylib, `notarytool` + staple, or Gatekeeper blocks the app. Windows: Authenticode cert (OV/EV, or Azure Trusted Signing) or SmartScreen warns on every download | M + recurring cost |
| F42 | `.github/workflows/ci.yml:8,14,23,28` | `ubuntu-latest` + `apt-get install libgtk-4-dev` | add `macos-14` (`brew install gtk4 pkgconf`) and `windows-latest` (`msys2/setup-msys2` with `mingw-w64-ucrt-x86_64-gtk4` and the `x86_64-pc-windows-gnu` toolchain; msvc needs gvsbuild-built GTK). Toolkit-free crates can be checked from Linux today with `rustup target add` (24 s warm) | M |
| F43 | `scripts/check_*.sh`, `site_shots.sh`, `*_helpers.py` | docker + headless sway, wtype/virtual-pointer, AT-SPI | Linux-only by design; macOS/Windows GUI checks would need their own harness (none exists) | L (if wanted) |

## Summary: work per platform

| Area | Windows | macOS |
|---|---|---|
| Toolkit-free crates compile | F1, F2, F3 (S each); F6 tests (S) | F6 two perf tests (S); otherwise already compiles |
| GTK apps compile | F4 (S), F5 fonts module gating (S) + whatever the native runners find | nothing found statically |
| Saving correct | F7 dir fsync (S), **F8 save over mapped file (M-L, Brick)** | F7 optional `F_FULLFSYNC` (S) |
| Data safety | F13 Slate sweep deletes live copies (S), F16/F17 state dir relative/temp (M) | F13 (S) |
| Recent files | F10 always empty (S), F11 `\\?\` + duplicates (S), F12 `~` label (S) | F11 case duplicates (S) |
| Per-user folders | F16-F18 one helper (M) | F16 decide XDG vs `~/Library` (S) |
| Fonts / docx fidelity | F5 embedded fonts (M), F37 metric fonts (M) | F5 (M), F37 (M) |
| Shortcuts | — | F28 (S), F29 (M), F30 (S) |
| Menus / launch | F33 filters (S) | F31 global menu bar (M), **F34 Finder open (M)**, F33 (S) |
| Theme | F32 (S) | F32 (S) |
| Packaging + signing | F38 (M), F39 installer + GTK runtime (L), F41 Authenticode (M + cost) | F38 (M), F39 `.app` bundle + relocation (L), F41 notarization (M + $99/yr) |
| CI | F42 MSYS2 job (M) | F42 macOS job (M) |
| Rough total | ~3-4 weeks to a signed installer, of which F8 and F39 dominate | ~3 weeks, of which F39/F41 and the ⌘ work dominate |

`suite-platform` should own: per-user folders (F16-F18, toolkit-free so engine crates can use
it), process liveness (F13, or suite-file), font registration and bundled metric fonts (F5,
F37; needs GTK/Pango, which suite-platform may depend on), the portal theme (F32), and the
primary-modifier helper (F29) if not suite-ui. Its `Platform` enum and `Unsupported` error
(`lib.rs:11-43`) gain `Windows`/`MacOs` when the first port starts.

## What runs

Nothing was run on Windows or macOS in this slice (static inventory + `cargo check` only).
No screenshots. On Linux, the 63-check matrix runs in ~24 s warm.

## Open questions

1. F8: does `MoveFileExW` over a file Brick has mapped really fail on Windows, and with which
   error? Needs a Windows runner (one test: open a CSV with `Source::open`, `save_atomic` over
   it). Answer decides whether ADR 0002 (mmap the source) needs a Windows variant.
2. F7: confirm `File::open(dir)` fails on Windows (expected: os error 5), i.e. every save
   currently reports an error there.
3. F2 on macOS: does the SIGBUS guard test pass (macOS gives `si_addr` for SIGBUS on a
   truncated mapping) [INFERENCE: yes, untested]?
4. F34 / F31: confirm GTK 4's macOS backend delivers Finder opens as `GApplication::open`
   and what app menu it adds by default.
5. F16: on macOS, XDG folders (`~/.local/state`) or Apple's `~/Library/Application Support`?
   A product decision for the ADR.
6. Exact GTK/Pango versions in Homebrew and MSYS2 today (F27; the native-build slices see them).
7. Windows target: `-gnu` via MSYS2 (packaged GTK, easiest) or `-msvc` via gvsbuild (native
   toolchain, more work)? The libraries check identically on both.
