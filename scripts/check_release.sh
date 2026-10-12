#!/usr/bin/env bash
# PKG-3 acceptance: the release binary starts on clean Arch, Ubuntu 24.04, and Fedora.
#
#   scripts/check_release.sh dist/brick-X.Y.Z-x86_64-linux/brick
#   scripts/check_release.sh dist/slate-X.Y.Z-x86_64-linux/slate
#   scripts/check_release.sh dist/frame-X.Y.Z-x86_64-linux/frame
#
# For each distro: a fresh container gets only what a user would install (GTK 4's runtime,
# from the distro's own packages), then the binary opens a small file under GTK's Broadway
# backend. Brick opens a CSV and must report its first frame and quit (`--bench-open`);
# Slate opens a Markdown file and must still be running 8 s later; Frame opens the fixture
# deck, draws its densest slide, and must report the frames drawn and quit
# (`--bench-draw`). That loads every library it links against and opens a window. Also
# checks that no glibc symbol it needs is newer than Ubuntu 24.04's 2.39. Needs docker.
set -euo pipefail
bin=$(realpath "${1:?usage: check_release.sh BINARY}")
app=$(basename "$bin")
GLIBC_FLOOR=2.39

newest=$(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -1)
if [ "$(printf '%s\n%s\n' "$newest" "$GLIBC_FLOOR" | sort -V | tail -1)" != "$GLIBC_FLOOR" ]; then
  echo "FAIL needs glibc $newest, newer than Ubuntu 24.04's $GLIBC_FLOOR"; exit 1
fi
echo "ok   glibc: newest symbol GLIBC_$newest (floor $GLIBC_FLOOR)"

case $app in
  brick) ext=csv; content='id,name\n1,a\n2,b\n'
    open="brick /tmp/t.csv --bench-open 2>&1"; opened='^OPENED ' ;;
  slate) ext=md; content='# A heading\n\nSome *text*.\n'
    # Still running when the timeout stops it: exit 124.
    open="slate /tmp/t.md 2>&1; echo exit \$?"; opened='^exit 124$' ;;
  frame) ext=pptx; deck="$(dirname "$0")/../apps/frame/crates/pptx/tests/data/fixture.pptx"
    open="frame /tmp/t.pptx --bench-draw 5 2>&1"; opened='^{"slide":' ;;
  *) echo "unknown app $app"; exit 1 ;;
esac
file=$(mktemp --suffix=".$ext"); trap 'rm -f "$file"' EXIT
if [ "$app" = frame ]; then cp "$deck" "$file"; else printf "$content" > "$file"; fi
chmod a+r "$file"

# distro, image, command installing GTK 4's runtime and its Broadway server
checks=(
  "Arch|archlinux:latest|pacman -Syu --noconfirm --needed gtk4 >/dev/null"
  "Ubuntu 24.04|ubuntu:24.04|apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends libgtk-4-1 libgtk-4-bin >/dev/null"
  "Fedora|fedora:latest|dnf install -y -q gtk4 >/dev/null"
)
fail=0
for c in "${checks[@]}"; do
  IFS='|' read -r name image install <<<"$c"
  limit=60; [ "$app" = slate ] && limit=8
  out=$(docker run --rm -v "$bin:/usr/local/bin/$app:ro" -v "$file:/tmp/t.$ext:ro" "$image" sh -c "
    $install || exit 3
    gtk4-broadwayd :5 >/dev/null 2>&1 & sleep 1
    GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 GDK_DEBUG=no-portals timeout $limit $open
  " 2>&1) && status=0 || status=$?
  if [ $status = 0 ] && grep -q "$opened" <<<"$out"; then
    case $app in
      brick) echo "ok   $name: $(grep '^FIRST_FRAME ' <<<"$out" | cut -c1-80)" ;;
      frame) echo "ok   $name: $(grep '^{"slide":' <<<"$out" | cut -c1-100)" ;;
      *) echo "ok   $name: $app opened t.$ext and was still running after ${limit}s" ;;
    esac
  else
    echo "FAIL $name (exit $status):"; tail -20 <<<"$out" | sed 's/^/     /'; fail=1
  fi
done
[ $fail = 0 ] && echo "RELEASE OK" || { echo "RELEASE CHECK FAILED"; exit 1; }
