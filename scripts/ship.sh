#!/usr/bin/env bash
# Release pipeline. Each step is separate so the ship-it skill can confirm
# with the user before anything leaves the machine. APP is brick (the default), slate,
# frame, or etch.
#
#   scripts/ship.sh [APP] preflight          checks only, changes nothing
#   scripts/ship.sh [APP] bump patch|minor|major|X.Y.Z
#   scripts/ship.sh [APP] build              release binary (built in Ubuntu 24.04) + tarball + latest.json in dist/
#   scripts/ship.sh [APP] verify             the binary starts on clean Arch, Ubuntu 24.04, and Fedora; the installer works there
#   scripts/ship.sh [APP] upload [--dry-run] tarball, checksum, latest.json -> Cloudflare R2
#   scripts/ship.sh [APP] site   [--dry-run] the app's download page (sites/APP) -> Cloudflare Pages
#   scripts/ship.sh suite-site [--dry-run]   suite install page (sites/cronkite) -> Cloudflare Pages
#   scripts/ship.sh video [--dry-run]        the explainer's English cuts (dist/video/, from
#                                            video/render.mjs) and a poster -> R2 video/ID/
#   scripts/ship.sh [APP] tag                annotated git tag (local only): vX.Y.Z for Brick,
#                                            APP-vX.Y.Z for the others (slate-v0.3.0)
#
# Brick's release sits at the bucket's root (latest.json, releases/), the other apps' under
# their name (slate/, frame/, etch/).
set -euo pipefail
cd "$(dirname "$0")/.."

[ -f .env.ship ] && set -a && . ./.env.ship && set +a
APP=brick
case "${1:-}" in brick|slate|frame|etch) APP=$1; shift ;; esac
TARGET=x86_64-unknown-linux-gnu
case $APP in
brick)
  ID=dev.cronkite.Brick  # desktop entry and icon names, as the app ID (APP_ID in apps/brick/src/main.rs)
  MANIFEST=Cargo.toml; SECTION=workspace.package  # Brick's version is the workspace's
  PREFIX=""; PAGES_VAR=CF_PAGES_PROJECT; TAG_PREFIX=v
  INSTALL_CHECK=scripts/check_install.sh
  ;;
slate)
  ID=dev.cronkite.Slate  # APP_ID in apps/slate/src/main.rs
  MANIFEST=apps/slate/Cargo.toml; SECTION=package
  PREFIX="slate/"; PAGES_VAR=CF_SLATE_PAGES_PROJECT; TAG_PREFIX=slate-v
  INSTALL_CHECK=scripts/check_install_slate.sh
  ;;
frame)
  ID=dev.cronkite.Frame  # APP_ID in apps/frame/src/main.rs
  MANIFEST=apps/frame/Cargo.toml; SECTION=package
  PREFIX="frame/"; PAGES_VAR=CF_FRAME_PAGES_PROJECT; TAG_PREFIX=frame-v
  INSTALL_CHECK=scripts/check_install_frame.sh
  ;;
etch)
  ID=dev.cronkite.Etch  # APP_ID in apps/etch/src/main.rs
  MANIFEST=apps/etch/Cargo.toml; SECTION=package
  PREFIX="etch/"; PAGES_VAR=CF_ETCH_PAGES_PROJECT; TAG_PREFIX=etch-v
  INSTALL_CHECK=scripts/check_install_etch.sh
  ;;
esac
# Where the build puts latest.json and the deploy its copy of the site: dist/ for Brick,
# dist/APP/ for the others.
OUT="dist${PREFIX:+/${PREFIX%/}}"
version() { sed -n "/^\[$SECTION\]/,/^\[/{s/^version = \"\(.*\)\"/\1/p}" "$MANIFEST"; }
V=$(version)
TAG="$TAG_PREFIX$V"
NAME="$APP-$V-x86_64-linux"
TAR="dist/$NAME.tar.gz"
DRY=false; [ "${2:-}" = "--dry-run" ] && DRY=true
BUILD_IMAGE=brick-build:ubuntu-24.04
need() { for v in "$@"; do [ -n "${!v:-}" ] || { echo "missing env: $v (see .env.ship.example)"; exit 1; }; done; }
run() { if $DRY; then echo "[dry-run] $*"; else "$@"; fi; }
wr() { npx --yes wrangler@4 "$@"; }
# Where the app's latest.json and releases/ are.
base() { echo "$DOWNLOAD_BASE_URL${PREFIX:+/${PREFIX%/}}"; }
# Create Pages project $1 unless it exists. Match the name exactly: the list's domains
# column holds brick.cronkite.cc.
pages_project() {
  if ! wr pages project list --json | python3 -c 'import json, sys
sys.exit(sys.argv[1] not in [p["Project Name"] for p in json.load(sys.stdin)])' "$1"; then
    # --force: wrangler 4.149 otherwise hands new projects to Workers and fails on a plain
    # folder of assets. Like `spreadsheet`, this is a classic Pages project.
    run wr pages project create "$1" --production-branch main --force
  fi
}

case "${1:-}" in
preflight)
  fail=0
  [ -z "$(git status --porcelain)" ] || { echo "FAIL working tree not clean"; fail=1; }
  b=$(git branch --show-current); [ "$b" = main ] || { echo "FAIL on branch $b, not main"; fail=1; }
  git rev-parse "$TAG" >/dev/null 2>&1 && { echo "FAIL tag $TAG already exists; bump first"; fail=1; }
  for f in packaging/$ID.desktop packaging/$ID.svg sites/$APP/index.html sites/$APP/install.sh "$INSTALL_CHECK"; do
    [ -f "$f" ] || { echo "FAIL $f missing: $APP has no packaging, download page, or install check yet"; fail=1; }
  done
  cargo fmt --all --check || { echo "FAIL fmt"; fail=1; }
  cargo clippy --workspace --all-targets --all-features -- -D warnings || { echo "FAIL clippy"; fail=1; }
  cargo test --workspace || { echo "FAIL tests"; fail=1; }
  command -v npx >/dev/null || { echo "FAIL npx not found (needed for wrangler)"; fail=1; }
  docker info >/dev/null 2>&1 || { echo "FAIL docker not usable (the release builds in a container)"; fail=1; }
  for v in CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID R2_BUCKET "$PAGES_VAR" DOWNLOAD_BASE_URL; do
    [ -n "${!v:-}" ] || { echo "FAIL env $v not set"; fail=1; }
  done
  echo "$APP version $V"; python3 scripts/backlog.py status
  [ $fail = 0 ] && echo "PREFLIGHT OK" || { echo "PREFLIGHT FAILED"; exit 1; }
  ;;
bump)
  new="${2:?usage: bump patch|minor|major|X.Y.Z}"
  IFS=. read -r ma mi pa <<<"$V"
  case "$new" in
    patch) new="$ma.$mi.$((pa+1))" ;; minor) new="$ma.$((mi+1)).0" ;; major) new="$((ma+1)).0.0" ;;
  esac
  sed -i "/^\[$SECTION\]/,/^\[/s/^version = \".*\"/version = \"$new\"/" "$MANIFEST"
  cargo check -q --workspace
  echo "$APP $V -> $new"
  ;;
build)
  need DOWNLOAD_BASE_URL
  # Built in Ubuntu 24.04 so it runs on that and anything newer (PKG-3); its own target
  # dir, so the host's builds stay as they are.
  docker build -q -t "$BUILD_IMAGE" -f packaging/build.Dockerfile packaging >/dev/null
  docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/src" -w /src \
    -e CARGO_HOME=/src/target/container/cargo -e CARGO_TARGET_DIR=/src/target/container \
    "$BUILD_IMAGE" cargo build --release --locked -p $APP --target $TARGET
  rm -rf "dist/$NAME"; mkdir -p "dist/$NAME"
  cp "target/container/$TARGET/release/$APP" "dist/$NAME/"
  strip "dist/$NAME/$APP" || true
  cp packaging/$ID.desktop packaging/$ID.svg "dist/$NAME/"
  [ -f apps/$APP/README.md ] && cp apps/$APP/README.md "dist/$NAME/"
  cp LICENSE* "dist/$NAME/" 2>/dev/null || true
  tar -C dist -czf "$TAR" "$NAME"
  (cd dist && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
  sha=$(cut -d' ' -f1 "$TAR.sha256"); size=$(stat -c%s "$TAR")
  mkdir -p "$OUT"
  cat > "$OUT/latest.json" <<JSON
{"version":"$V","date":"$(date -u +%Y-%m-%d)","url":"$(base)/releases/v$V/$NAME.tar.gz","sha256":"$sha","bytes":$size}
JSON
  echo "built $TAR ($size bytes, sha256 $sha)"
  ;;
verify)
  [ -x "dist/$NAME/$APP" ] || { echo "run build first"; exit 1; }
  scripts/check_release.sh "dist/$NAME/$APP"
  "$INSTALL_CHECK" "$TAR"
  ;;
upload)
  need CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID R2_BUCKET
  [ -f "$TAR" ] || { echo "run build first"; exit 1; }
  run wr r2 object put "$R2_BUCKET/${PREFIX}releases/v$V/$NAME.tar.gz" --file "$TAR" --content-type application/gzip --remote
  run wr r2 object put "$R2_BUCKET/${PREFIX}releases/v$V/$NAME.tar.gz.sha256" --file "$TAR.sha256" --content-type text/plain --remote
  run wr r2 object put "$R2_BUCKET/${PREFIX}latest.json" --file "$OUT/latest.json" --content-type application/json --remote
  echo "check: curl -fsSI $(base)/releases/v$V/$NAME.tar.gz"
  ;;
site)
  need CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID "$PAGES_VAR"
  [ -f "$OUT/latest.json" ] || { echo "run build first"; exit 1; }
  rm -rf "$OUT/site"; cp -r "sites/$APP" "$OUT/site"
  # The page shows the release written into it, not a latest.json beside it: Pages fails
  # to publish a .json file in the slate project ("Unknown internal error", 2026-10-12).
  python3 -c 'import sys; p, rel = sys.argv[1], open(sys.argv[2]).read().strip()
html = open(p).read(); assert "@RELEASE_JSON@" in html, p
open(p, "w").write(html.replace("@RELEASE_JSON@", rel.replace("</", "<\\/")))' "$OUT/site/index.html" "$OUT/latest.json"
  # The installer reads latest.json from the bucket (PKG-1).
  need DOWNLOAD_BASE_URL
  sed -i "s|@DOWNLOAD_BASE_URL@|$(base)|" "$OUT/site/install.sh"
  pages_project "${!PAGES_VAR}"
  run wr pages deploy "$OUT/site" --project-name "${!PAGES_VAR}" --branch main --commit-dirty=true
  ;;
suite-site)
  # cronkite.cc: static, no release data, so it deploys only when sites/cronkite changes.
  # The custom domain cronkite.cc is attached to the project once, in the dashboard.
  need CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID CF_SUITE_PAGES_PROJECT
  rm -rf dist/cronkite-site; cp -r sites/cronkite dist/cronkite-site
  pages_project "$CF_SUITE_PAGES_PROJECT"
  run wr pages deploy dist/cronkite-site --project-name "$CF_SUITE_PAGES_PROJECT" --branch main --commit-dirty=true
  ;;
video)
  # The explainer video (video/render.mjs and --cut short) and a poster from the short
  # cut's title card, under video/ID/ where ID is the two cuts' hash: a new render gets new
  # addresses, so a page linking the old ones never shows a mix. Captions are burned in.
  need CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID R2_BUCKET DOWNLOAD_BASE_URL
  full=dist/video/cronkite-explainer.mp4 short=dist/video/cronkite-explainer-short.mp4
  for f in "$full" "$short"; do
    [ -f "$f" ] || { echo "no $f (cd video && node render.mjs, and --cut short)"; exit 1; }
  done
  poster=dist/video/cronkite-explainer-short.jpg
  ffmpeg -hide_banner -loglevel error -y -ss 1 -i "$short" -frames:v 1 -q:v 3 "$poster"
  id=$(cat "$full" "$short" | sha256sum | cut -c1-12)
  run wr r2 object put "$R2_BUCKET/video/$id/$(basename "$full")" --file "$full" --content-type video/mp4 --remote
  run wr r2 object put "$R2_BUCKET/video/$id/$(basename "$short")" --file "$short" --content-type video/mp4 --remote
  run wr r2 object put "$R2_BUCKET/video/$id/$(basename "$poster")" --file "$poster" --content-type image/jpeg --remote
  echo "video base: $DOWNLOAD_BASE_URL/video/$id"
  ;;
tag)
  git tag -a "$TAG" -m "$TAG"
  echo "tagged $TAG locally; push with: git push origin main $TAG"
  ;;
*) sed -n '2,17p' "$0"; exit 1 ;;
esac
