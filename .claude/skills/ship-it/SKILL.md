---
name: ship-it
description: Cut a release of a Cronkite app (Brick, the spreadsheet, by default; or Slate, the word processor) and deploy it to Cloudflare: preflight checks, version bump, release build, tarball upload to R2, download page to Cloudflare Pages, git tag. Use whenever the user says "ship it", "release", "deploy", "cut a build", "publish a version", "push to Cloudflare", or asks to put a new build online, even if they don't name Cloudflare.
---

# ship-it

The app is a native Linux binary, so Cloudflare hosts the release, not the app itself:

- **R2** stores the tarball, its `.sha256`, and `latest.json` (version, URL, checksum).
- **Pages** serves the app's download page (`sites/brick/` at brick.cronkite.cc, `sites/slate/` at slate.cronkite.cc), which reads `latest.json`.

Every step runs through `scripts/ship.sh`. Anything that leaves the machine (upload, site deploy, push) needs an explicit yes from the user in chat first, every time.

Which app: ask if the user didn't say. For Slate, put `slate` before every command (`scripts/ship.sh slate build`); without it, the commands act on Brick. Slate's version is `apps/slate/Cargo.toml`'s (Brick's is the workspace's), its release lives under `slate/` in the bucket, its Pages project is `CF_SLATE_PAGES_PROJECT` (created on the first `site` deploy if missing), and its tags are `slate-vX.Y.Z`; `vX.Y.Z` below means that tag. Its CHANGELOG heading is `## Slate vX.Y.Z (YYYY-MM-DD)`.

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
scripts/ship.sh build    # compiles in an Ubuntu 24.04 container (needs docker)
scripts/ship.sh verify   # starts the binary on clean Arch, Ubuntu 24.04, and Fedora
```

Check the output: tarball exists, size is plausible, `dist/<name>/<app> --version` prints the new version, and `verify` ends with `RELEASE OK` and `INSTALL OK`. Stop on any FAIL. Report size and sha256.

## 4. Dry run, then confirm

```sh
scripts/ship.sh upload --dry-run
scripts/ship.sh site --dry-run
```

Show the user what will happen in this shape and wait for a clear yes:

```
Ready to ship vX.Y.Z
- Upload brick-X.Y.Z-x86_64-linux.tar.gz (N MB) + checksum + latest.json to R2 bucket <bucket>
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

- `curl -fsSI <download base>/releases/vX.Y.Z/<name>.tar.gz` returns 200 (the base is `$DOWNLOAD_BASE_URL`, plus `/slate` for Slate).
- `curl -fsS <pages URL from wrangler output>/ | grep -c '"version":"X.Y.Z"'` is 1: the deploy writes `latest.json` into the page (Pages fails to publish a `.json` file in the `slate` project).
- `curl -fsS <pages URL>/install.sh | grep -c "<download base>"` is 1 (the deploy filled in the download URL).
- Downloaded tarball matches the sha256.
- Optionally, on this machine: `curl -fsSL <pages URL>/install.sh | sh` reports the new version (in a throwaway `HOME`, `XDG_CONFIG_HOME`, and `XDG_DATA_HOME` to leave this machine's defaults alone).

If upload succeeds but the site deploy fails, the old page still points at the old version; rerun `site` alone after fixing. If a step fails mid-way, do not delete uploaded objects; report what landed.

## 6. Push

Ask before running `git push origin main vX.Y.Z`.

## 7. Report

```
Shipped vX.Y.Z
Download: <url>  (N MB, sha256 <first 12 chars>)
Page: <pages url>
Stories: <IDs>
Not done: <push pending | none>
```

The download page also serves `install.sh` (story PKG-1), which reads `latest.json`, so a site deploy is all a new version needs; nothing goes to a package registry.

The suite page at cronkite.cc (`sites/cronkite/`) is not part of a release: it links to the apps' installers and holds no version. Deploy it only when it changes, with the same yes first: `scripts/ship.sh suite-site --dry-run`, then `scripts/ship.sh suite-site` (Pages project `CF_SUITE_PAGES_PROJECT`). The cronkite.cc custom domain is attached to that project once, in the Cloudflare dashboard; so is slate.cronkite.cc to Slate's project.
