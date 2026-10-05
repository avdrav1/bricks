---
name: ship-it
description: Cut a release of the spreadsheet app and deploy it to Cloudflare: preflight checks, version bump, release build, tarball upload to R2, download page to Cloudflare Pages, git tag. Use whenever the user says "ship it", "release", "deploy", "cut a build", "publish a version", "push to Cloudflare", or asks to put a new build online, even if they don't name Cloudflare.
---

# ship-it

The app is a native Linux binary, so Cloudflare hosts the release, not the app itself:

- **R2** stores the tarball, its `.sha256`, and `latest.json` (version, URL, checksum).
- **Pages** serves the download page in `site/`, which reads `latest.json`.

Every step runs through `scripts/ship.sh`. Anything that leaves the machine (upload, site deploy, push) needs an explicit yes from the user in chat first, every time.

## 0. One-time setup

If `.env.ship` is missing, stop and walk the user through it:

1. Copy `.env.ship.example` to `.env.ship`; confirm `.env.ship` is in `.gitignore`.
2. In the Cloudflare dashboard: create the R2 bucket, enable public access (custom domain or r2.dev), create a Pages project with the name in `CF_PAGES_PROJECT` (or let the first `wrangler pages deploy` create it).
3. Create an API token with Pages Edit and R2 Storage Edit. The user pastes it into `.env.ship` themselves.

Never print, echo, log, or commit token values. Never ask the user to paste a token into chat.

## 1. Preflight

```sh
scripts/ship.sh preflight
```

Stop on any FAIL and report it. Common fixes: commit or stash changes, switch to `main`, bump the version if the tag exists. Do not "fix" failing tests by skipping them.

## 2. Pick the version

Propose a bump from what shipped since the last tag (`git log $(git describe --tags --abbrev=0 2>/dev/null)..HEAD --oneline`; story IDs are in commit subjects):

- `0.0.x` patch: before V0.1, any internal build.
- `minor`: a milestone gate passed (M3 gate = `0.1.0`).
- `patch` after 0.1.0: fixes only.

Confirm with the user, then:

```sh
scripts/ship.sh bump <patch|minor|major|X.Y.Z>
```

Write the release notes into `CHANGELOG.md` under `## vX.Y.Z (YYYY-MM-DD)`, one line per story (`- ENG-4: Auto-detect delimiter`). Commit: `release: vX.Y.Z`.

## 3. Build

```sh
scripts/ship.sh build
```

Check the output: tarball exists, size is plausible, and the binary runs: `dist/<name>/spreadsheet --version` or a smoke launch. Report size and sha256.

## 4. Dry run, then confirm

```sh
scripts/ship.sh upload --dry-run
scripts/ship.sh site --dry-run
```

Show the user what will happen in this shape and wait for a clear yes:

```
Ready to ship vX.Y.Z
- Upload spreadsheet-X.Y.Z-x86_64-linux.tar.gz (N MB) + checksum + latest.json to R2 bucket <bucket>
- Deploy download page to Pages project <project>
- Tag vX.Y.Z locally
Proceed?
```

## 5. Ship

After the yes, in order:

```sh
scripts/ship.sh upload
scripts/ship.sh site
scripts/ship.sh tag
```

Then verify, and report each check:

- `curl -fsSI $DOWNLOAD_BASE_URL/releases/vX.Y.Z/<name>.tar.gz` returns 200.
- `curl -fsS <pages URL from wrangler output>/latest.json` shows the new version.
- Downloaded tarball matches the sha256.

If upload succeeds but the site deploy fails, the old page still points at the old version; rerun `site` alone after fixing. If a step fails mid-way, do not delete uploaded objects; report what landed.

## 6. Push

Ask before running `git push origin main vX.Y.Z`.

## 7. Report

```
Shipped vX.Y.Z
Download: <url>  (N MB, sha256 <first 12 chars>)
Page: <pages url>
Stories: <IDs>
Not done: <push pending | AUR update pending | none>
```

For AUR (story PKG-1 and later), print the new `pkgver` and `sha256sums` for `packaging/aur/PKGBUILD` and let the user publish it; do not push to the AUR.
