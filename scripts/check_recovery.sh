#!/usr/bin/env bash
# APP-7 acceptance: relaunch after a crash offers to restore edits.
#
#   scripts/check_recovery.sh [BINARY]   (default target/release/spreadsheet)
#
# In an Arch container under headless sway, with real key presses (wtype) and a real
# crash (kill -9 after the journal is written):
#   1. start window: edit a file, crash; relaunch without a file; Enter on the focused
#      Restore; Ctrl+S. The saved file has the edit; no journal is left.
#   2. reopen: edit, crash; relaunch on the file; "Restore unsaved edits?" defaults to
#      Restore: Enter; Ctrl+S. The saved file has the edit.
#   3. changed file: edit, crash, then the file changes; relaunch on it. The edits must not
#      go onto it: Enter keeps the journal for later, and the file is as changed.
#   4. untitled: Ctrl+N, type, crash; relaunch; Enter on Restore. The restored window
#      holds the value (it journals it again).
# Needs docker. VERBOSE=1 prints the transcript.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/spreadsheet}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p spreadsheet)"; exit 1; }

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/spreadsheet:ro" "${shots[@]}" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus jq >/dev/null 2>&1
mkdir -p /shots
mkdir -p /tmp/rt && chmod 700 /tmp/rt
export XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
dir=/tmp/state/spreadsheet/recovery
printf "output HEADLESS-1 resolution 1280x720\ndefault_border none\n" > /tmp/sway.conf
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1)) SWAYSOCK=$(ls /tmp/rt/sway-ipc.*.sock)
  titles() { swaymsg -t get_tree | jq -r ".. | objects | select(.type==\"con\" and .name!=null) | .name" | sort | tr "\n" ","; }
  journals() { ls $dir/*.journal 2>/dev/null | wc -l; }
  crash() { sleep 6.5; echo "journals before crash: $(journals)"; pkill -9 -x spreadsheet; sleep 1; }
  start() { spreadsheet "$@" >>/tmp/app.log 2>&1 & sleep 3; }
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  # The headless seat has no keyboard: each wtype adds a virtual one, and focus arrives
  # with it. Keys sent in the same instant can be lost, so wait before and after.
  keys() { wtype -s 400 "$@" -s 400; }

  echo "== 1 start window"
  printf "id,name\n1,a\n2,b\n" > /tmp/one.csv
  start /tmp/one.csv; keys first-edit; keys -k Return; crash
  start; echo "start window: $(titles)"; shot 1-start
  keys -k Return; sleep 2; echo "after restore: $(titles)"; shot 1-restored
  keys -M ctrl s -m ctrl; sleep 2
  echo "saved: $(tr "\n" " " < /tmp/one.csv)"
  sleep 1; echo "journals after save: $(journals)"
  pkill -x spreadsheet; sleep 1

  echo "== 2 reopen"
  printf "id,name\n1,a\n2,b\n" > /tmp/two.csv
  start /tmp/two.csv; keys second-edit; keys -k Return; crash
  start /tmp/two.csv; sleep 1; echo "dialog: $(titles)"; shot 2-dialog
  keys -k Return; sleep 2; echo "after answer: $(titles)"; shot 2-answered
  keys -M ctrl s -m ctrl; sleep 2
  echo "saved: $(tr "\n" " " < /tmp/two.csv)"
  pkill -x spreadsheet; sleep 1

  echo "== 3 changed file"
  printf "id,name\n1,a\n2,b\n" > /tmp/three.csv
  start /tmp/three.csv; keys lost-edit; keys -k Return; crash
  sleep 1; echo "3,c" >> /tmp/three.csv
  start /tmp/three.csv; sleep 1; keys -k Return; sleep 1
  echo "file: $(tr "\n" " " < /tmp/three.csv)"
  pkill -x spreadsheet; sleep 1
  echo "journals kept: $(journals)"
  rm -f $dir/*.journal

  echo "== 4 untitled"
  start; keys -M ctrl n -m ctrl; sleep 1; keys untitled-value; keys -k Return; crash
  old=$(ls $dir/*.journal)
  start; keys -k Return; sleep 2; echo "after restore: $(titles)"; shot 4-restored
  sleep 6.5
  echo "crashed journal gone: $([ -e "$old" ] && echo no || echo yes)"
  echo "rejournaled: $(grep -l untitled-value $dir/*.journal 2>/dev/null | grep -vc "$old")"
  pkill -x spreadsheet
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); dir=$dir; session"
echo "== app log"; grep "^spreadsheet:" /tmp/app.log | head -20
' 2>&1) || true
[ -n "${VERBOSE:-}" ] && sed 's/^/  /' <<<"$out"
fail=0
check() { # WHAT PATTERN
  if grep -qE -- "$2" <<<"$out"; then echo "ok   $1"; else echo "FAIL $1 (no match for: $2)"; fail=1; fi
}
check "1: a journal is on disk when it crashes" "^journals before crash: 1$"
check "1: the start window offers it" "^start window: Spreadsheet,$"
check "1: Restore opens the file" "^after restore: • one.csv,$"
check "1: the saved file has the edit" "^saved: id,name first-edit,a 2,b $"
check "1: no journal once saved" "^journals after save: 0$"
check "2: reopening asks first" "^dialog: .*two.csv"
check "2: the saved file has the edit" "^saved: id,name second-edit,a 2,b $"
check "3: a changed file doesn't get the edits" "^file: id,name 1,a 2,b 3,c $"
check "3: the journal is kept for later" "^journals kept: 1$"
check "4: Restore puts the untitled table back" "^crashed journal gone: yes$"
check "4: the restored table holds the value" "^rejournaled: 1$"
if [ $fail = 0 ]; then echo "RECOVERY OK"; else
  [ -n "${VERBOSE:-}" ] || sed 's/^/  /' <<<"$out" | tail -40
  echo "RECOVERY CHECK FAILED"; exit 1
fi
