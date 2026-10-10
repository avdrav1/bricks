#!/usr/bin/env bash
# PKG-3 acceptance: the release binary starts on clean Arch, Ubuntu 24.04, and Fedora.
#
#   scripts/check_release.sh dist/spreadsheet-X.Y.Z-x86_64-linux/spreadsheet
#
# For each distro: a fresh container gets only what a user would install (GTK 4's runtime,
# from the distro's own packages), then the binary opens a small CSV under GTK's Broadway
# backend and must report its first frame and quit (`--bench-open`). That loads every
# library it links against and draws a window. Also checks that no glibc symbol it needs
# is newer than Ubuntu 24.04's 2.39. Needs docker.
set -euo pipefail
bin=$(realpath "${1:?usage: check_release.sh BINARY}")
GLIBC_FLOOR=2.39

newest=$(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -1)
if [ "$(printf '%s\n%s\n' "$newest" "$GLIBC_FLOOR" | sort -V | tail -1)" != "$GLIBC_FLOOR" ]; then
  echo "FAIL needs glibc $newest, newer than Ubuntu 24.04's $GLIBC_FLOOR"; exit 1
fi
echo "ok   glibc: newest symbol GLIBC_$newest (floor $GLIBC_FLOOR)"

csv=$(mktemp --suffix=.csv); trap 'rm -f "$csv"' EXIT
printf 'id,name\n1,a\n2,b\n' > "$csv"; chmod a+r "$csv"

# distro, image, command installing GTK 4's runtime and its Broadway server
checks=(
  "Arch|archlinux:latest|pacman -Syu --noconfirm --needed gtk4 >/dev/null"
  "Ubuntu 24.04|ubuntu:24.04|apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends libgtk-4-1 libgtk-4-bin >/dev/null"
  "Fedora|fedora:latest|dnf install -y -q gtk4 >/dev/null"
)
fail=0
for c in "${checks[@]}"; do
  IFS='|' read -r name image install <<<"$c"
  out=$(docker run --rm -v "$bin:/usr/local/bin/spreadsheet:ro" -v "$csv:/tmp/t.csv:ro" "$image" sh -c "
    $install || exit 3
    gtk4-broadwayd :5 >/dev/null 2>&1 & sleep 1
    GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 GDK_DEBUG=no-portals timeout 60 \
      spreadsheet /tmp/t.csv --bench-open 2>&1
  " 2>&1) && status=0 || status=$?
  if [ $status = 0 ] && grep -q '^OPENED ' <<<"$out"; then
    echo "ok   $name: $(grep '^FIRST_FRAME ' <<<"$out" | cut -c1-80)"
  else
    echo "FAIL $name (exit $status):"; tail -20 <<<"$out" | sed 's/^/     /'; fail=1
  fi
done
[ $fail = 0 ] && echo "RELEASE OK" || { echo "RELEASE CHECK FAILED"; exit 1; }
