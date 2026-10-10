#!/usr/bin/env bash
# APP-8 acceptance: for a malformed file, a dialog shows the line number and the offending
# text.
#
#   scripts/check_malformed.sh [BINARY]   (default target/release/spreadsheet)
#
# In an Arch container under headless sway; the dialog's text is read through the
# accessibility tree (scripts/a11y_helpers.py), as GTK shows it:
#   1. a quote that never closes, after a quoted newline: the dialog names line 5 and shows
#      its text.
#   2. 200k rows, text after a quote on 7 of them: the dialog lists the first lines and
#      counts the rest. "Go to Line 70,002" puts the cursor there: typing and Ctrl+S
#      change that line. Saving doesn't ask again.
#   3. a binary file is refused: "Cannot open", with the line and the bytes.
#   4. a well-formed file opens with no dialog.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/spreadsheet}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p spreadsheet)"; exit 1; }

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/spreadsheet:ro" \
  -v "$PWD/scripts/a11y_helpers.py:/a11y.py:ro" "${shots[@]}" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core >/dev/null 2>&1
mkdir -p /shots /tmp/rt && chmod 700 /tmp/rt
export XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1280x720\ndefault_border none\n" > /tmp/sway.conf
python3 - <<EOF
rows = ["id,name"]
for i in range(200_000):
    rows.append(f"{i},\"name {i}\"" + ("x" if i in (10, 70_000, 70_001, 140_000, 150_000, 190_000, 199_999) else ""))
open("/tmp/many.csv", "w").write("\n".join(rows) + "\n")
EOF
printf "id,name,note\n1,\"Smith, J\",\"two\nlines\"\n2,Jones,ok\n3,\"Lee, K,never closed\n4,x,y\n" > /tmp/unclosed.csv
printf "PK\003\004\0\0\024\0\010\0data.csvxxxxxxxxxxxxxxxx\n" > /tmp/archive.csv
printf "id,name\n1,\"quoted, fine\"\n2,\"say \"\"hi\"\"\"\n" > /tmp/clean.csv
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  start() { spreadsheet "$@" >>/tmp/app.log 2>&1 & sleep 4; }
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  # The headless seat has no keyboard: each wtype adds a virtual one, and focus arrives
  # with it. Keys sent in the same instant can be lost, so wait before and after.
  keys() { wtype -s 400 "$@" -s 400; }
  shown() { python3 /a11y.py text | sed "s/^/${1:-shown}: /"; }

  echo "== 1 unclosed"
  start /tmp/unclosed.csv; shot 1-unclosed; shown
  pkill -x spreadsheet; sleep 1

  echo "== 2 many"
  start /tmp/many.csv; shot 2-many; shown
  python3 /a11y.py press "Go to Line 12" && echo "pressed"; sleep 1
  keys here; keys -k Return; keys -M ctrl s -m ctrl; sleep 3; shot 2-saved
  echo "line 12: $(sed -n 12p /tmp/many.csv)"
  echo "line 13: $(sed -n 13p /tmp/many.csv)"
  shown after
  pkill -x spreadsheet; sleep 1

  echo "== 3 binary"
  start /tmp/archive.csv; shot 3-binary; shown
  pkill -x spreadsheet; sleep 1

  echo "== 4 clean"
  start /tmp/clean.csv; shot 4-clean; shown
  pkill -x spreadsheet
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); session" 2>/tmp/dbus.log
echo "== app log"; grep "^spreadsheet:" /tmp/app.log | head -20
' 2>&1) || true
[ -n "${VERBOSE:-}" ] && sed 's/^/  /' <<<"$out"
section() { awk -v s="== $1" '/^== /{on=($0 ~ "^" s)} on' <<<"$out"; }
fail=0
check() { # WHAT SECTION PATTERN
  if section "$2" | grep -qE -- "$3"; then echo "ok   $1"; else echo "FAIL $1 (no match for: $3)"; fail=1; fi
}
refute() { # WHAT SECTION PATTERN
  if section "$2" | grep -qE -- "$3"; then echo "FAIL $1 (matched: $3)"; fail=1; else echo "ok   $1"; fi
}
check "1: the dialog names the file" "1 unclosed" "^shown: “unclosed.csv” has a malformed line$"
check "1: it gives the line (quoted newline counted)" "1 unclosed" "Line 5: a quote opens a field and never closes"
check "1: it shows the offending text" "1 unclosed" '^shown:     3,"Lee, K,never closed$'
check "2: the first problem's line and text" "2 many" "Line 12: text follows a closing quote"
check "2: later lines across chunks" "2 many" "Line 70,003: text follows a closing quote"
check "2: the rest are counted" "2 many" "…and 4 more problems\."
check "2: Go to Line puts the cursor on that line" "2 many" '^line 12: here,'
check "2: and nowhere else" "2 many" '^line 13: 11,"name 11"$'
refute "2: saving doesn't ask again" "2 many" "^after: .*malformed"
check "3: a binary file is refused" "3 binary" "^shown: Cannot open .*archive.csv"
check "3: with the line and the bytes" "3 binary" 'binary data, not text\. Line 1 has a zero byte: “PK.*\\0\\0'
refute "4: a well-formed file opens with no dialog" "4 clean" "malformed"
if [ $fail = 0 ]; then echo "MALFORMED OK"; else
  echo "MALFORMED CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
