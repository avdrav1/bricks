#!/usr/bin/env bash
# PKG-7 acceptance on Linux: piping cronkite.cc's install.sh into sh installs the whole
# suite per user on clean Arch, Ubuntu 24.04, and Fedora by running each app's own
# installer, or only the apps named; keeps going past one app's failure and reports each
# app's result; and uninstalls them all.
#
#   scripts/check_install_suite.sh
#
# Serves a throwaway suite site on localhost: sites/cronkite/install.sh, and each app's
# sites/<app>/install.sh under <app>/ (CRONKITE_SITE_BASE), which download the live
# releases (BRICK_DOWNLOAD_URL and the others, from DL, default https://dl.cronkite.cc);
# and a broken site whose Frame has no installer. Per distro, in a fresh container as a
# normal user:
#   1. `sh -s -- slate paint`: refused (no app called paint), nothing installed;
#   2. without GTK 4, `sh -s -- brick`: Brick installed, the summary says it needs GTK 4,
#      exit 2;
#   3. with GTK 4, `sh -s -- slate frame`: exactly Slate and Frame installed, exit 0;
#   4. the broken site with Etch's download URL pointing nowhere, `sh -s -- brick frame
#      etch`: Brick installed, Frame (no installer) and Etch (no release) failed and
#      reported, exit 1;
#   5. `sh`: all four installed at the live versions ("Cronkite: 4 of 4 apps installed"),
#      each starts (`--version`) and has its menu entry;
#   6. `sh -s -- --uninstall`: all four removed ("4 of 4 apps removed"), nothing of theirs
#      left: binaries, menu entries, icons, licences, file defaults.
# Needs docker, curl, and python3. VERBOSE=1 prints each container's transcript; ONLY=Fedora
# (or Arch, Ubuntu) runs one distro.
set -euo pipefail
cd "$(dirname "$0")/.."
DL=${DL:-https://dl.cronkite.cc}
APPS="brick slate frame etch"
declare -A NAME=([brick]=Brick [slate]=Slate [frame]=Frame [etch]=Etch)
# Where each app's release is: Brick's at the bucket's root, the others' under their name.
dl() { if [ "$1" = brick ]; then echo "$DL"; else echo "$DL/$1"; fi; }
declare -A VERSION
for a in $APPS; do
  VERSION[$a]=$(curl -fsSL "$(dl "$a")/latest.json" | python3 -c 'import json, sys; print(json.load(sys.stdin)["version"])')
done

site=$(mktemp -d); trap 'kill $server 2>/dev/null || true; rm -rf "$site"' EXIT
cp sites/cronkite/install.sh "$site/install.sh"
envs=""
for a in $APPS; do
  mkdir -p "$site/$a"; cp "sites/$a/install.sh" "$site/$a/install.sh"
  [ "$a" = frame ] || { mkdir -p "$site/broken/$a"; cp "sites/$a/install.sh" "$site/broken/$a/install.sh"; }
  envs="$envs ${a^^}_DOWNLOAD_URL=$(dl "$a")"
done
chmod -R a+rX "$site"
port=$((20000 + RANDOM % 20000))
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$site" >/dev/null 2>&1 &
server=$!
sleep 1

# distro | image | GTK 4 runtime
checks=(
  "Arch|archlinux:latest|pacman -Syu --noconfirm --needed gtk4 >/dev/null"
  "Ubuntu 24.04|ubuntu:24.04|apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends libgtk-4-1 shared-mime-info >/dev/null"
  "Fedora|fedora:latest|dnf install -y -q gtk4 shared-mime-info >/dev/null"
)
fail=0
for c in "${checks[@]}"; do
  IFS='|' read -r distro image gtk <<<"$c"
  [ -n "${ONLY:-}" ] && [[ "$distro" != "$ONLY"* ]] && continue
  case "$image" in
    archlinux*) prep="pacman -Sy --noconfirm --needed curl >/dev/null" ;;
    ubuntu*) prep="apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends curl ca-certificates >/dev/null" ;;
    fedora*) prep="dnf install -y -q curl tar gzip util-linux >/dev/null" ;; # as on Workstation
  esac
  out=$(docker run --rm --network host "$image" sh -c "
    set -e
    $prep
    useradd -m tester
    as() { su tester -c \"cd; \$1\"; }
    url=http://127.0.0.1:$port
    suite=\"curl -fsSL \$url/install.sh | CRONKITE_SITE_BASE=\$url $envs sh\"
    # What of each app is installed: its binary, menu entry, icon, and licences.
    left='for a in $APPS; do for f in ~/.local/bin/\$a ~/.local/share/doc/\$a ~/.local/share/applications/dev.cronkite.*.desktop ~/.local/share/icons/hicolor/scalable/apps/dev.cronkite.*.svg; do [ -e \"\$f\" ] && echo \"left:\$f\"; done; done | sort -u; echo defaults:\$(grep -c dev.cronkite ~/.config/mimeapps.list 2>/dev/null || true)'
    bins='echo bins:\$(cd ~/.local/bin 2>/dev/null && ls | tr \"\\\\n\" \" \")'

    echo '== bad name'
    as \"\$suite -s -- slate paint\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as \"\$bins\"

    echo '== no gtk'
    as \"\$suite -s -- brick\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as 'rm -rf ~/.local ~/.config'

    echo '== subset'
    $gtk
    as \"\$suite -s -- slate frame\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as \"\$bins\"

    echo '== one fails'
    as \"curl -fsSL \$url/install.sh | CRONKITE_SITE_BASE=\$url/broken $envs ETCH_DOWNLOAD_URL=\$url/nowhere sh -s -- brick frame etch\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"

    echo '== all'
    as \"\$suite\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as \"\$bins\"
    for a in $APPS; do as \"echo version: \\\$(~/.local/bin/\$a --version)\"; done
    as 'echo entries: \$(ls ~/.local/share/applications | tr \"\\\\n\" \" \")'

    echo '== uninstall'
    as \"\$suite -s -- --uninstall\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as \"\$left\"
    as \"\$bins\"
  " 2>&1) || true
  ok=true
  check() { grep -q -- "$1" <<<"$out" || { echo "FAIL $distro: $2"; ok=false; }; }
  # The text between "== $1" and the next "== " heading of ours.
  part() { awk -v h="== $1" '$0 == h { on = 1; next } /^== (bad name|no gtk|subset|one fails|all|uninstall)$/ { on = 0 } on' <<<"$out"; }
  in_part() { grep -q -- "$2" <<<"$(part "$1")" || { echo "FAIL $distro ($1): $3"; ok=false; }; }
  in_part "bad name" "^cronkite install: no app called paint" "an unknown app not refused"
  in_part "bad name" "^exit 1$" "an unknown app didn't fail"
  in_part "bad name" "^bins:$" "an unknown app installed something"
  in_part "no gtk" "^Installed Brick ${VERSION[brick]}$" "Brick not installed without GTK 4"
  in_part "no gtk" "^Cronkite: 1 of 1 app installed; needs GTK 4: Brick$" "no GTK 4 summary"
  in_part "no gtk" "^exit 2$" "without GTK 4 the exit status isn't 2"
  in_part "subset" "^Installed Slate ${VERSION[slate]}$" "Slate not installed"
  in_part "subset" "^Installed Frame ${VERSION[frame]}$" "Frame not installed"
  in_part "subset" "^Cronkite: 2 of 2 apps installed$" "no subset summary"
  in_part "subset" "^exit 0$" "the subset didn't exit 0"
  in_part "subset" "^bins:frame slate$" "the subset installed other apps"
  in_part "one fails" "^Installed Brick ${VERSION[brick]}$" "Brick not installed beside the failures"
  in_part "one fails" "^cronkite install: cannot reach http://127.0.0.1:$port/broken/frame/install.sh$" "Frame's missing installer not reported"
  in_part "one fails" "^etch install: cannot reach http://127.0.0.1:$port/nowhere/latest.json$" "Etch's failing download not reported"
  in_part "one fails" "^  Brick  installed$" "Brick's result not listed"
  in_part "one fails" "^  Frame  failed (see above)$" "Frame's result not listed"
  in_part "one fails" "^  Etch   failed (see above)$" "Etch's result not listed"
  in_part "one fails" "^Cronkite: 1 of 3 apps installed; failed: Frame, Etch$" "no failure summary"
  in_part "one fails" "^exit 1$" "a failure didn't exit 1"
  for a in $APPS; do
    n=${NAME[$a]}
    in_part "all" "^\(Installed\|Updated\) $n \(.* -> \)\?${VERSION[$a]}$" "$n not installed"
    in_part "all" "^version: $a ${VERSION[$a]}$" "$n doesn't start at ${VERSION[$a]}"
    in_part "all" "^entries: .*dev.cronkite.$n.desktop" "$n has no menu entry"
    in_part "all" "^  $(printf '%-6s' "$n") installed$" "$n's result not listed"
    in_part "uninstall" "^Removed $n\." "$n not removed"
    in_part "uninstall" "^  $(printf '%-6s' "$n") removed$" "$n's removal not listed"
  done
  in_part "all" "^Cronkite: 4 of 4 apps installed$" "no install summary"
  in_part "all" "^exit 0$" "install all didn't exit 0"
  in_part "all" "^bins:brick etch frame slate$" "install all didn't install the four"
  in_part "uninstall" "^Cronkite: 4 of 4 apps removed$" "no uninstall summary"
  in_part "uninstall" "^exit 0$" "uninstall didn't exit 0"
  in_part "uninstall" "^defaults:0$" "uninstall left the apps' file defaults"
  in_part "uninstall" "^bins:$" "uninstall left binaries"
  check "^== uninstall$" "the container didn't get to the end"
  if grep -q '^left:' <<<"$(part uninstall)"; then echo "FAIL $distro: uninstall left $(grep '^left:' <<<"$out" | tr '\n' ' ')"; ok=false; fi
  if $ok; then
    echo "ok   $distro: refuses an unknown app, installs one (says it needs GTK 4 without it), two, and all four (${VERSION[brick]}, ${VERSION[slate]}, ${VERSION[frame]}, ${VERSION[etch]}), reports two failures beside an install, uninstalls all"
    [ -n "${VERBOSE:-}" ] && sed 's/^/     /' <<<"$out"
  else
    sed 's/^/     /' <<<"$out" | tail -80; fail=1
  fi
done
[ $fail = 0 ] && echo "SUITE INSTALL OK" || { echo "SUITE INSTALL CHECK FAILED"; exit 1; }
