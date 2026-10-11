#!/usr/bin/env bash
# FML-4 acceptance (and FML-11's find): formulas in a CSV show their results; typed
# formulas evaluate, undo removes them, and one that doesn't parse is refused with where
# it fails; the file keeps its formulas' text.
#
#   scripts/check_formulas.sh [BINARY]   (default target/release/bricks)
#
# In an Arch container under headless sway; the cursor cell and what it shows are read
# from the grid's accessible description (`B2: 4`) through scripts/a11y_helpers.py.
# /tmp/t.csv is `a,b\n2,=A2*2\n`:
#   1. the cursor on B2 shows `B2: 4`;
#   2. typing =A2*3 in C2 shows `C2: 6`;
#   3. Ctrl+Z empties C2 again: `C2: `;
#   4. typing =A2+ is refused: an alert says "Formula error at character 5";
#   5. after Escape and Ctrl+S the file still holds `=A2*2`;
#   6. (FML-11) Ctrl+F 4 finds B2, whose result is 4 (its text is =A2*2).
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/bricks}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p bricks)"; exit 1; }

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/bricks:ro" \
  -v "$PWD/scripts/a11y_helpers.py:/a11y.py:ro" "${shots[@]}" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core >/dev/null 2>&1
mkdir -p /shots /tmp/rt && chmod 700 /tmp/rt
export XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1280x720\ndefault_border none\n" > /tmp/sway.conf
printf "a,b\n2,=A2*2\n" > /tmp/t.csv
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  # The headless seat has no keyboard: each wtype adds a virtual one, and focus arrives
  # with it. Keys sent in the same instant can be lost, so wait before and after.
  keys() { wtype -s 400 "$@" -s 400; }
  shown() { python3 /a11y.py text | sed "s/^/${1:-shown}: /"; }

  bricks /tmp/t.csv >>/tmp/app.log 2>&1 & sleep 4
  echo "== 1 result"
  keys -k Right; shot 1-result; shown

  echo "== 2 typed"
  keys -k Right; keys "=A2*3"; keys -k Return; keys -k Up; shot 2-typed; shown

  echo "== 3 undo"
  keys -M ctrl z -m ctrl; sleep 1; shot 3-undo; shown

  echo "== 4 refused"
  keys "=A2+"; keys -k Return; sleep 1; shot 4-refused; shown

  echo "== 5 saved"
  keys -k Escape; keys -k Escape; keys -M ctrl s -m ctrl; sleep 3; shot 5-saved
  echo "file: $(tr "\n" "|" < /tmp/t.csv)"

  echo "== 6 find"
  keys -k Left; keys -k Left; shown before
  keys -M ctrl f -m ctrl; keys 4; keys -k Return; sleep 2; keys -k Escape; sleep 1
  shot 6-find; shown

  pkill -x bricks
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); session" 2>/tmp/dbus.log
echo "== app log"; grep "^bricks:" /tmp/app.log | head -20
' 2>&1) || true
[ -n "${VERBOSE:-}" ] && sed 's/^/  /' <<<"$out"
section() { awk -v s="== $1" '/^== /{on=($0 ~ "^" s)} on' <<<"$out"; }
fail=0
check() { # WHAT SECTION PATTERN
  if section "$2" | grep -qE -- "$3"; then echo "ok   $1"; else echo "FAIL $1 (no match for: $3)"; fail=1; fi
}
check "1: B2 shows its formula's result" "1 result" "^shown: B2: 4$"
check "2: a typed formula shows its result" "2 typed" "^shown: C2: 6$"
check "3: Ctrl+Z removes it" "3 undo" "^shown: C2: $"
check "4: a formula with an error is refused, saying where" "4 refused" "Formula error at character 5"
check "5: the file keeps the formula's text" "5 saved" "^file: a,b\|2,=A2\*2\|$"
check "6: find finds a formula's result" "6 find" "^shown: B2: 4$"
if [ $fail = 0 ]; then echo "FORMULAS OK"; else
  echo "FORMULAS CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
