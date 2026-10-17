#!/usr/bin/env bash
# PKG-7 acceptance on a clean macOS runner (install-suite.yml, Homebrew moved out of the
# way): piping cronkite.cc's install.sh into sh installs the whole suite into
# ~/Applications by running each app's own installer, or only the apps named; keeps going
# past one app's failure and reports each app's result; and uninstalls them all.
#
#   scripts/check_mac_install_suite.sh      (macOS only)
#
# A throwaway suite site on localhost serves sites/cronkite/install.sh and each app's
# sites/<app>/install.sh under <app>/ (CRONKITE_SITE_BASE), which download the live
# releases (BRICK_DOWNLOAD_URL and the others, from DL, default https://dl.cronkite.cc):
#   1. `sh -s -- slate paint`: refused, nothing installed;
#   2. `sh -s -- slate frame`: exactly Slate.app and Frame.app, exit 0;
#   3. Etch's download URL pointing nowhere, `sh -s -- brick etch`: Brick installed, Etch
#      failed and reported, exit 1;
#   4. `sh`: all four .app bundles at the live versions, each starting (`--version`),
#      "Cronkite: 4 of 4 apps installed";
#   5. `sh -s -- --uninstall`: "4 of 4 apps removed", none left in ~/Applications.
# Transcripts land in $OUT/logs (default out/mac-install-suite), the result lines in
# $OUT/summary.txt; a failure exits 1 after every step has run.
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out=${OUT:-out/mac-install-suite}
mkdir -p "$out/logs" "$out/site"
out=$(cd "$out" && pwd)
logs=$out/logs site=$out/site
: >"$out/summary.txt"
failed=0
note() { echo "$*" | tee -a "$out/summary.txt"; }
pass() { note "ok   $*"; }
fail() { note "FAIL $*"; failed=1; }
check() { if eval "$1"; then pass "$2"; else fail "$2"; fi; }

DL=${DL:-https://dl.cronkite.cc}
APPS="brick slate frame etch"
title() { case $1 in brick) echo Brick ;; slate) echo Slate ;; frame) echo Frame ;; etch) echo Etch ;; esac; }
dl() { if [ "$1" = brick ]; then echo "$DL"; else echo "$DL/$1"; fi; }
version() { curl -fsSL "$(dl "$1")/latest.json" | sed -n 's/.*"version" *: *"\([^"]*\)".*/\1/p'; }

if [[ -e /opt/homebrew/lib ]]; then
  fail "Homebrew is at /opt/homebrew: this check needs a Mac without it"
else
  pass "no Homebrew at /opt/homebrew ($(sw_vers -productVersion), $(uname -m))"
fi

cp "$root/sites/cronkite/install.sh" "$site/install.sh"
for a in $APPS; do
  mkdir -p "$site/$a"; cp "$root/sites/$a/install.sh" "$site/$a/install.sh"
  export "$(echo "$a" | tr a-z A-Z)_DOWNLOAD_URL=$(dl "$a")"
done
port=$((20000 + RANDOM % 20000))
url=http://127.0.0.1:$port
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$site" >"$logs/http.log" 2>&1 &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT
for _ in $(seq 20); do curl -fsS -o /dev/null "$url/install.sh" 2>/dev/null && break; sleep 0.5; done
export CRONKITE_SITE_BASE=$url

# The Cronkite apps in ~/Applications, by name.
apps_in() { (cd ~/Applications 2>/dev/null && ls -d Brick.app Slate.app Frame.app Etch.app 2>/dev/null | tr '\n' ' ' | sed 's/ $//'); }
# Run the suite installer as the page says, with arguments $2...; transcript in $logs/$1.txt,
# exit status in $status.
suite() {
  local tag=$1; shift
  curl -fsSL "$url/install.sh" | sh -s -- "$@" >"$logs/$tag.txt" 2>&1
  status=$?
  sed "s/^/     $tag: /" "$logs/$tag.txt"
}
has() { grep -q -- "$2" "$logs/$1.txt"; }

check '[ -z "$(apps_in)" ]' "no Cronkite apps in ~/Applications to start with"

suite bad-name slate paint
check 'has bad-name "^cronkite install: no app called paint" && [ "$status" = 1 ] && [ -z "$(apps_in)" ]' \
  "an unknown app is refused (exit $status) and nothing installed"

suite subset slate frame
check 'has subset "^Installed Slate $(version slate)$" && has subset "^Installed Frame $(version frame)$" && has subset "^Cronkite: 2 of 2 apps installed$" && [ "$status" = 0 ]' \
  "sh -s -- slate frame installs Slate and Frame (exit $status)"
check '[ "$(apps_in)" = "Frame.app Slate.app" ]' "only Slate and Frame are in ~/Applications: $(apps_in)"

ETCH_DOWNLOAD_URL=$url/nowhere
suite one-fails brick etch
ETCH_DOWNLOAD_URL=$(dl etch)
check 'has one-fails "^Installed Brick $(version brick)$" && has one-fails "^etch install: cannot reach $url/nowhere/latest.json$" && has one-fails "^  Etch   failed (see above)$" && has one-fails "^Cronkite: 1 of 2 apps installed; failed: Etch$" && [ "$status" = 1 ]' \
  "with Etch's release unreachable, Brick installs and Etch's failure is reported (exit $status)"

suite all
check 'has all "^Cronkite: 4 of 4 apps installed$" && [ "$status" = 0 ]' "sh installs all four (exit $status)"
check '[ "$(apps_in)" = "Brick.app Etch.app Frame.app Slate.app" ]' "~/Applications has $(apps_in)"
for a in $APPS; do
  n=$(title "$a"); v=$(version "$a")
  got=$(~/Applications/"$n".app/Contents/MacOS/"$a" --version 2>&1)
  check '[ "$got" = "$a $v" ] && has all "^  $(printf "%-6s" "$n") installed$"' "$n.app starts: $got"
done

suite uninstall --uninstall
check 'has uninstall "^Cronkite: 4 of 4 apps removed$" && [ "$status" = 0 ]' "sh -s -- --uninstall removes all four (exit $status)"
check '[ -z "$(apps_in)" ]' "no Cronkite apps left in ~/Applications"

if [ $failed = 0 ]; then note "check_mac_install_suite: OK"; else note "check_mac_install_suite: FAILED"; exit 1; fi
