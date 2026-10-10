#!/usr/bin/env bash
# APP-6 acceptance: the app follows the desktop's dark/light preference, live, and its
# text is not blurry at 1.5x and 2x.
#
#   scripts/check_look.sh [BINARY]   (default target/release/spreadsheet)
#
# In Arch (newest GTK: the app sets gtk-interface-color-scheme) and Ubuntu 24.04 (GTK
# 4.14: the app sets the older dark-variant switch) containers:
#   - theme: a stand-in settings portal (look_helpers.py portal) says prefer-dark, then
#     prefer-light, then prefer-dark again while the app runs (SettingChanged, as when a
#     user flips the desktop's switch). The grid must turn dark, light, dark: background
#     and text brightness measured on screenshots.
#   - scale: headless sway's output goes 1x, 1.5x, 2x with the app running; X11 runs at
#     GDK_SCALE=1 and 2. At each scale the grid's cell text must be at least as crisp as at
#     1x, and much crisper than the 1x window scaled up (what a blurry window looks like).
#     The measure: of pixels half-way to ink, the share that is solid ink.
# Needs docker. VERBOSE=1 prints the measurements; SHOTS=DIR keeps the screenshots there;
# ONLY=Arch or ONLY=Ubuntu runs one.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/spreadsheet}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p spreadsheet)"; exit 1; }

# What runs inside each container, once its packages are in.
scenario='
set -euo pipefail
mkdir -p /tmp/rt /shots && chmod 700 /tmp/rt
# The stand-in portal must be the only one: no activating the real one in its place.
rm -f /usr/share/dbus-1/services/org.freedesktop.portal.Desktop.service
export XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state
printf "id,name,city\n1,Portland Oregon,Denver\n2,Boston Massachusetts,Austin Texas\n3,Chicago,Miami\n" > /tmp/t.csv
session() {
  h=/scripts/look_helpers.py
  echo 1 > /tmp/scheme
  python3 $h portal /tmp/scheme & portal=$!
  printf "output HEADLESS-1 resolution 1920x1080 scale 1\ndefault_border none\n" > /tmp/sway.conf
  # --unsupported-gpu: sway 1.9 refuses to start when the host has the Nvidia module
  # loaded, though the headless, software-rendered output here never touches a GPU.
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  sock=$(ls /tmp/rt/wayland-[0-9] 2>/dev/null | head -1) || true
  [ -n "$sock" ] || { echo "app: sway did not start: $(tail -3 /tmp/sway.log)"; }
  export WAYLAND_DISPLAY=$(basename "$sock") SWAYSOCK=$(ls /tmp/rt/sway-ipc.*.sock)
  GSK_DEBUG=renderer GDK_BACKEND=wayland spreadsheet /tmp/t.csv >/tmp/app.log 2>&1 & app=$!; sleep 3
  grim /shots/dark.png
  echo 2 > /tmp/scheme; kill -USR1 $portal; sleep 1.5; grim /shots/light.png
  echo 1 > /tmp/scheme; kill -USR1 $portal; sleep 1.5; grim /shots/dark-again.png
  echo 2 > /tmp/scheme; kill -USR1 $portal; sleep 1.5
  for s in 1 1.5 2; do
    swaymsg output HEADLESS-1 scale $s >/dev/null; sleep 2; grim /shots/wayland-$s.png
  done
  sed -n "s/^.*Gtk-WARNING.*/app: &/p" /tmp/app.log | head -3
  grep -m1 -iE "using .*renderer|renderer is|Not using" /tmp/app.log | sed "s/^/renderer: /" || true
  kill $app; swaymsg exit >/dev/null 2>&1 || true; kill $portal
  unset WAYLAND_DISPLAY
  for s in 1 2; do
    Xvfb :9 -screen 0 $((1280 * s))x$((800 * s))x24 >/dev/null 2>&1 & x=$!; sleep 1
    DISPLAY=:9 GDK_BACKEND=x11 GDK_SCALE=$s spreadsheet /tmp/t.csv >/dev/null 2>&1 & app=$!; sleep 3
    python3 $h grab :9 /shots/x11-$s.png; kill $app $x; sleep 0.5
  done
  python3 $h theme /shots/dark.png /shots/light.png /shots/dark-again.png
  python3 $h sharp /shots/wayland-1.png 1.5 /shots/wayland-1.5.png
  python3 $h sharp /shots/wayland-1.png 2 /shots/wayland-2.png
  python3 $h sharp /shots/x11-1.png 2 /shots/x11-2.png
}
dbus-run-session -- bash -c "$(declare -f session); session"
'

fail=0
check() { # NAME IMAGE INSTALL [RENDERER]
  local name=$1 image=$2 install=$3 renderer=${GSK_RENDERER:-${4:-}} shots=() out
  [ -n "${ONLY:-}" ] && [[ "$name" != "$ONLY"* ]] && return
  if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS/$name"; shots=(-v "$(realpath "$SHOTS/$name"):/shots"); fi
  # Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
  # GSK_RENDERER picks GTK's renderer (cairo, ngl, vulkan); set, it overrides the default.
  out=$(docker run --rm --cap-add SYS_NICE -e GSK_RENDERER="$renderer" -v "$bin:/usr/local/bin/spreadsheet:ro" \
    -v "$PWD/scripts:/scripts:ro" "${shots[@]}" "$image" \
    bash -c "$install >/dev/null 2>&1; $scenario" 2>&1) || true
  [ -n "${VERBOSE:-}" ] && grep -E "^/shots|^app:" <<<"$out" | sed "s/^/  $name /"
  local ok=true
  bad() { echo "FAIL $name $*"; ok=false; fail=1; }
  if grep -q "^app:" <<<"$out"; then bad "the app printed a GTK warning: $(grep -m1 '^app:' <<<"$out")"; fi
  # Theme: background dark (< 0.3) with light text, then light (> 0.8) with dark text.
  for shot in dark light dark-again; do
    local line bg ink want
    line=$(grep "^/shots/$shot.png:" <<<"$out" || true)
    bg=$(sed -n 's/.*background \([0-9.]*\).*/\1/p' <<<"$line"); ink=$(sed -n 's/.*text \([0-9.]*\).*/\1/p' <<<"$line")
    [ -n "$bg" ] || { bad "$shot: no measurement"; continue; }
    if [[ $shot == dark* ]]; then want="bg < 0.3 and ink > 0.7"; else want="bg > 0.8 and ink < 0.3"; fi
    python3 -c "bg, ink = $bg, $ink; exit(0 if $want else 1)" || bad "$shot: grid background $bg, text $ink (want $want)"
  done
  # Sharpness: at least 95% of the 1x crispness, and at least twice the upscaled 1x.
  # The 1x text must itself measure as text (>= 0.3), so an empty shot can't pass.
  local summary=""
  for shot in wayland-1.5 wayland-2 x11-2; do
    local line got base up
    line=$(grep "^/shots/$shot.png:" <<<"$out" || true)
    read -r got base up < <(sed -n 's/.*: \([0-9.]*\) base \([0-9.]*\) upscaled \([0-9.]*\)/\1 \2 \3/p' <<<"$line") || true
    [ -n "${got:-}" ] || { bad "$shot: no measurement"; continue; }
    python3 -c "exit(0 if $base >= 0.3 and $got >= 0.95 * $base and $got >= 2 * $up else 1)" \
      || bad "$shot: text crispness $got (1x: $base; 1x scaled up: $up)"
    summary+=" $shot $got (1x $base, upscaled $up);"
  done
  if $ok; then echo "ok   $name${renderer:+ ($renderer)}: dark, light, dark followed live; crisp text:$summary"
  else tail -20 <<<"$out" | sed 's/^/     /'; fi
}

check Arch archlinux:latest "pacman -Syu --noconfirm --needed sway grim xorg-server-xvfb gtk4 \
  ttf-dejavu dbus python-gobject python-pillow python-numpy vulkan-swrast mesa"
# GTK 4.14 without a GPU falls back to its cairo renderer, which draws fractional scales
# at the next whole scale and lets the compositor shrink them: softer at 1.5x, in every
# GTK 4.14 app. Desktops with any GPU get the GL renderer, which this checks.
check "Ubuntu 24.04" ubuntu:24.04 "apt-get update -qq && DEBIAN_FRONTEND=noninteractive \
  apt-get install -y -qq --no-install-recommends sway grim xvfb libgtk-4-1 fonts-dejavu-core \
  dbus dbus-x11 at-spi2-core python3-gi python3-pil python3-numpy mesa-vulkan-drivers" ngl
if [ $fail = 0 ]; then echo "LOOK OK"; else echo "LOOK CHECK FAILED"; exit 1; fi
