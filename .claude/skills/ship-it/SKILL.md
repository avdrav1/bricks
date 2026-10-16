---
name: ship-it
description: Cut a release of a Cronkite app (Brick, the spreadsheet; Slate, the word processor; Frame, the presentation app; or Etch, the image editor) for Linux and macOS and deploy it to Cloudflare: preflight checks, version bump, release builds (Linux in a container, macOS on GitHub's macos-14 runner), tarball and DMG upload to R2, download page to Cloudflare Pages, git tag. Use whenever the user says "ship it", "release", "deploy", "cut a build", "publish a version", "push to Cloudflare", or asks to put a new build online, even if they don't name Cloudflare.
---

# ship-it

The app is a native Linux binary and a macOS app bundle, so Cloudflare hosts the release, not the app itself:

- **R2** stores the Linux tarball, the macOS tarball and DMG (Apple Silicon, MAC-7), their `.sha256` files, and `latest.json` (version, each platform's URL and checksum).
- **Pages** serves the app's download page (`sites/<app>/` at `<app>.cronkite.cc`), which shows the release in `latest.json` and offers the visitor's platform first.

Every step runs through `scripts/ship.sh`. Anything that leaves the machine (upload, site deploy, push) needs an explicit yes from the user in chat first, every time.

Which app: ask if the user didn't say. Put the app before every command (`scripts/ship.sh etch build`); without one, the commands act on Brick. Each app's release differs only in these:

| App | Version in | Bucket prefix | Pages project | Tag | CHANGELOG heading | Release smoke (`verify`) |
| --- | --- | --- | --- | --- | --- | --- |
| Brick | `Cargo.toml` `[workspace.package]` | none | `CF_PAGES_PROJECT` | `vX.Y.Z` | `## vX.Y.Z (YYYY-MM-DD)` | `--bench-open` on a CSV; `check_install.sh` |
| Slate | `apps/slate/Cargo.toml` | `slate/` | `CF_SLATE_PAGES_PROJECT` | `slate-vX.Y.Z` | `## Slate vX.Y.Z (YYYY-MM-DD)` | still running on a .md; `check_install_slate.sh` |
| Frame | `apps/frame/Cargo.toml` | `frame/` | `CF_FRAME_PAGES_PROJECT` | `frame-vX.Y.Z` | `## Frame vX.Y.Z (YYYY-MM-DD)` | `--bench-draw` on the fixture deck; `check_install_frame.sh` |
| Etch | `apps/etch/Cargo.toml` | `etch/` | `CF_ETCH_PAGES_PROJECT` | `etch-vX.Y.Z` | `## Etch vX.Y.Z (YYYY-MM-DD)` | `--bench-stroke`; `check_install_etch.sh` |

Below, `vX.Y.Z` means the app's tag and the download base is `$DOWNLOAD_BASE_URL` plus the bucket prefix. The macOS release of every app is built and checked by `.github/workflows/release-macos.yml` on GitHub (needs `gh`, logged in); its verify job is the macOS release smoke. Pages projects are created on the first `site` deploy if missing; each `<app>.cronkite.cc` custom domain is attached to its project once, in the Cloudflare dashboard. An app ships only once it has its desktop entry and icon (`packaging/dev.cronkite.<App>.*`), its download page and installer (`sites/<app>/`), and its install check; `preflight` fails without them. An app's first release also deploys the suite page (below), which lists it: deploy it after the app's own page, so its install command works by then.

## 0. One-time setup

If `.env.ship` is missing, stop and walk the user through it:

1. Copy `.env.ship.example` to `.env.ship`; confirm `.env.ship` is in `.gitignore`.
2. In the Cloudflare dashboard: create the R2 bucket, enable public access (custom domain or r2.dev), create a Pages project with the name in the app's Pages variable (or let the first `site` deploy create it).
3. Create an API token with Pages Edit and R2 Storage Edit. The user pastes it into `.env.ship` themselves.

Never print, echo, log, or commit token values. Never ask the user to paste a token into chat.

## 1. Preflight

```sh
scripts/ship.sh <app> preflight
```

Stop on any FAIL and report it. Common fixes: commit or stash changes, switch to `main`, bump the version if the tag exists. Do not "fix" failing tests by skipping them.

## 2. Pick the version

Propose a bump from what shipped since the last tag (`git log $(git describe --tags --abbrev=0 2>/dev/null)..HEAD --oneline`; story IDs are in commit subjects):

- `0.0.x` patch: before the app's first milestone release, any internal build.
- `minor`: a milestone gate passed (Brick's M3 gate = `0.1.0`; an app's first gated release is its `0.1.0`).
- `patch` after 0.1.0: fixes only.

Confirm with the user, then:

```sh
scripts/ship.sh bump <patch|minor|major|X.Y.Z>
```

Write the release notes into `CHANGELOG.md` under the app's heading from the table, newest first, one line per story (`- ENG-4: Auto-detect delimiter`). Commit: `release: vX.Y.Z`.

## 3. Build

```sh
scripts/ship.sh <app> build    # compiles in an Ubuntu 24.04 container (needs docker)
scripts/ship.sh <app> verify   # starts the binary on clean Arch, Ubuntu 24.04, and Fedora; runs the installer there
```

Check the output: tarball exists, size is plausible, `dist/<name>/<app> --version` prints the new version, and `verify` ends with `RELEASE OK` and `INSTALL OK`. Stop on any FAIL. Report size and sha256.

Then the macOS build. It runs on GitHub at the release commit, so that commit must be on GitHub first: ask before pushing it (`git push origin main`; the tag comes later).

```sh
scripts/ship.sh <app> build --macos   # dispatches release-macos.yml at HEAD, waits, then fetch-macos
```

`release-macos.yml` builds the app's ad-hoc-signed `<App>.app` (GTK inside, `scripts/mac_bundle.sh`), its DMG, and the tarball the installer takes, then on a fresh runner with Homebrew moved away installs it with the curl installer and from the DMG and starts it (`scripts/check_mac_install.sh`). `fetch-macos` only takes a green run at HEAD: it puts `dist/<app>-X.Y.Z-arm64-macos.{tar.gz,dmg}` and their `.sha256` files in `dist/` and rewrites `latest.json` with both platforms. To release all four apps at once, run the workflow once (`gh workflow run release-macos.yml --ref main`, its `apps` input defaults to all four), watch it (`gh run watch RUN_ID --exit-status`), then `scripts/ship.sh <app> fetch-macos RUN_ID` per app. On an arm64 Mac with Homebrew's `gtk4 adwaita-icon-theme librsvg dylibbundler`, `build --macos` builds locally instead.

Check: the run's URL, its `verify` job's `check_mac_install: OK`, the run summary's sizes, and the screenshots in its `macos-install` artifact (`gh run download RUN_ID -n macos-install`). Report the DMG's size and sha256.

## 4. Dry run, then confirm

```sh
scripts/ship.sh <app> upload --dry-run
scripts/ship.sh <app> site --dry-run
```

Show the user what will happen in this shape and wait for a clear yes:

```
Ready to ship vX.Y.Z
- Upload <app>-X.Y.Z-x86_64-linux.tar.gz (N MB), <app>-X.Y.Z-arm64-macos.tar.gz (N MB) and .dmg (N MB) + checksums + latest.json to R2 bucket <bucket>/<prefix>
- Deploy download page to Pages project <project>
- Tag vX.Y.Z locally
Proceed?
```

A release without the macOS files (`upload` says "Linux only") still ships; its `latest.json` then has no `macos-arm64`, and the Mac installer says there's no macOS release yet.

## 5. Ship

After the yes, in order:

```sh
scripts/ship.sh <app> upload
scripts/ship.sh <app> site
scripts/ship.sh <app> tag
```

Then verify, and report each check:

- `curl -fsSI <download base>/releases/vX.Y.Z/<name>.tar.gz` returns 200, and so do the macOS tarball and DMG.
- `curl -fsS <pages URL from wrangler output>/ | grep -c '"version":"X.Y.Z"'` is 1: the deploy writes `latest.json` into the page (Pages fails to publish a `.json` file in the `slate` project).
- `curl -fsS <pages URL>/install.sh | grep -c "<download base>"` is 1 (the deploy filled in the download URL).
- Downloaded tarball and DMG match their sha256.
- Optionally, on this machine: `curl -fsSL <pages URL>/install.sh | sh` reports the new version (in a throwaway `HOME`, `XDG_CONFIG_HOME`, and `XDG_DATA_HOME` to leave this machine's defaults alone).

If upload succeeds but the site deploy fails, the old page still points at the old version; rerun `site` alone after fixing. If a step fails mid-way, do not delete uploaded objects; report what landed.

## 6. Push

Ask before running `git push origin main vX.Y.Z`.

## 7. Report

```
Shipped vX.Y.Z
Download: <url>  (N MB, sha256 <first 12 chars>)
Mac DMG: <url>  (N MB, sha256 <first 12 chars>; release-macos.yml run <url>)
Page: <pages url>
Stories: <IDs>
Not done: <push pending | none>
```

The download page also serves `install.sh` (stories PKG-1, MAC-7), which reads `latest.json`: the top-level `url` and `sha256` on Linux (installers from before MAC-7 read only those, so `latest.json` keeps them, last on its line), `platforms.macos-arm64` on a Mac. So a site deploy is all a new version needs; nothing goes to a package registry. Pushing a release tag runs `release-macos.yml` again for that app; that run is a check, not the release.

The suite page at cronkite.cc (`sites/cronkite/`) is not part of a release: it links to the apps' installers and holds no version. Deploy it only when it changes (an app's first release adds its row), with the same yes first: `scripts/ship.sh suite-site --dry-run`, then `scripts/ship.sh suite-site` (Pages project `CF_SUITE_PAGES_PROJECT`).
