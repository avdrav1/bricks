#!/usr/bin/env bash
# Release pipeline. Each step is separate so the ship-it skill can confirm
# with the user before anything leaves the machine.
#
#   scripts/ship.sh preflight          checks only, changes nothing
#   scripts/ship.sh bump patch|minor|major|X.Y.Z
#   scripts/ship.sh build              release binary + tarball + latest.json in dist/
#   scripts/ship.sh upload [--dry-run] tarball, checksum, latest.json -> Cloudflare R2
#   scripts/ship.sh site   [--dry-run] download page -> Cloudflare Pages
#   scripts/ship.sh tag                annotated git tag vX.Y.Z (local only)
set -euo pipefail
cd "$(dirname "$0")/.."

[ -f .env.ship ] && set -a && . ./.env.ship && set +a
APP=spreadsheet
TARGET=x86_64-unknown-linux-gnu
version() { sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml; }
V=$(version)
NAME="$APP-$V-x86_64-linux"
TAR="dist/$NAME.tar.gz"
DRY=false; [ "${2:-}" = "--dry-run" ] && DRY=true
need() { for v in "$@"; do [ -n "${!v:-}" ] || { echo "missing env: $v (see .env.ship.example)"; exit 1; }; done; }
run() { if $DRY; then echo "[dry-run] $*"; else "$@"; fi; }
wr() { npx --yes wrangler@4 "$@"; }

case "${1:-}" in
preflight)
  fail=0
  [ -z "$(git status --porcelain)" ] || { echo "FAIL working tree not clean"; fail=1; }
  b=$(git branch --show-current); [ "$b" = main ] || { echo "FAIL on branch $b, not main"; fail=1; }
  git rev-parse "v$V" >/dev/null 2>&1 && { echo "FAIL tag v$V already exists; bump first"; fail=1; }
  cargo fmt --all --check || { echo "FAIL fmt"; fail=1; }
  cargo clippy --workspace --all-targets -- -D warnings || { echo "FAIL clippy"; fail=1; }
  cargo test --workspace || { echo "FAIL tests"; fail=1; }
  command -v npx >/dev/null || { echo "FAIL npx not found (needed for wrangler)"; fail=1; }
  for v in CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID R2_BUCKET CF_PAGES_PROJECT DOWNLOAD_BASE_URL; do
    [ -n "${!v:-}" ] || { echo "FAIL env $v not set"; fail=1; }
  done
  echo "version $V"; python3 scripts/backlog.py status
  [ $fail = 0 ] && echo "PREFLIGHT OK" || { echo "PREFLIGHT FAILED"; exit 1; }
  ;;
bump)
  new="${2:?usage: bump patch|minor|major|X.Y.Z}"
  IFS=. read -r ma mi pa <<<"$V"
  case "$new" in
    patch) new="$ma.$mi.$((pa+1))" ;; minor) new="$ma.$((mi+1)).0" ;; major) new="$((ma+1)).0.0" ;;
  esac
  sed -i "/^\[workspace.package\]/,/^\[/s/^version = \".*\"/version = \"$new\"/" Cargo.toml
  cargo check -q --workspace
  echo "$V -> $new"
  ;;
build)
  need DOWNLOAD_BASE_URL
  cargo build --release --locked -p $APP --target $TARGET
  rm -rf "dist/$NAME"; mkdir -p "dist/$NAME"
  cp "target/$TARGET/release/$APP" "dist/$NAME/"
  strip "dist/$NAME/$APP" || true
  cp packaging/$APP.desktop README.md "dist/$NAME/"
  cp LICENSE* "dist/$NAME/" 2>/dev/null || true
  tar -C dist -czf "$TAR" "$NAME"
  (cd dist && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
  sha=$(cut -d' ' -f1 "$TAR.sha256"); size=$(stat -c%s "$TAR")
  cat > dist/latest.json <<JSON
{"version":"$V","date":"$(date -u +%Y-%m-%d)","url":"$DOWNLOAD_BASE_URL/releases/v$V/$NAME.tar.gz","sha256":"$sha","bytes":$size}
JSON
  echo "built $TAR ($size bytes, sha256 $sha)"
  ;;
upload)
  need CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID R2_BUCKET
  [ -f "$TAR" ] || { echo "run build first"; exit 1; }
  run wr r2 object put "$R2_BUCKET/releases/v$V/$NAME.tar.gz" --file "$TAR" --content-type application/gzip --remote
  run wr r2 object put "$R2_BUCKET/releases/v$V/$NAME.tar.gz.sha256" --file "$TAR.sha256" --content-type text/plain --remote
  run wr r2 object put "$R2_BUCKET/latest.json" --file dist/latest.json --content-type application/json --remote
  echo "check: curl -fsSI $DOWNLOAD_BASE_URL/releases/v$V/$NAME.tar.gz"
  ;;
site)
  need CLOUDFLARE_API_TOKEN CLOUDFLARE_ACCOUNT_ID CF_PAGES_PROJECT
  [ -f dist/latest.json ] || { echo "run build first"; exit 1; }
  rm -rf dist/site; cp -r site dist/site; cp dist/latest.json dist/site/
  run wr pages deploy dist/site --project-name "$CF_PAGES_PROJECT" --branch main --commit-dirty=true
  ;;
tag)
  git tag -a "v$V" -m "v$V"
  echo "tagged v$V locally; push with: git push origin main v$V"
  ;;
*) sed -n '2,12p' "$0"; exit 1 ;;
esac
