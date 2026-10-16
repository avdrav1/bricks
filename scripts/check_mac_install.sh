#!/usr/bin/env bash
# MAC-7 acceptance on a clean macOS runner (release-macos.yml's verify job, Homebrew moved
# out of the way): each app installs from its curl installer and from its DMG, and starts
# without Homebrew.
#
#   scripts/check_mac_install.sh DIST [APPS]    (macOS only; APPS: "brick slate frame etch")
#
# DIST holds scripts/mac_bundle.sh's APP-V-arm64-macos.{tar.gz,dmg}. A throwaway download
# site on localhost serves each app's installer, a latest.json naming the macOS tarball,
# and a tampered copy. Per app:
#   1. the installer refuses the tampered tarball and installs nothing;
#   2. `curl …/install.sh | sh` (APP_DOWNLOAD_URL pointing at the site) installs
#      ~/Applications/<Name>.app: no quarantine flag, a valid signature, registered with
#      Launch Services, which lists it for the app's sample file;
#   3. the installed app starts on the sample file and shows a window, loading every
#      library from its bundle and none from /opt/homebrew (DYLD_PRINT_LIBRARIES); it says
#      it runs from the bundle, writes gdk-pixbuf's loader list and fontconfig's cache into
#      ~/Library/Caches/<app id>, and its ~/Library folder for settings and state
#      (Application Support; Frame keeps only Caches); About shows its icon;
#   4. `open -a` on another file starts it through Launch Services;
#   5. --uninstall removes the app and its registration;
#   6. its DMG mounts with <Name>.app and an Applications link; the app copied from it into
#      /Applications starts on the sample file.
# Then Gatekeeper: Etch from the DMG with a browser's quarantine flag, and again after the
# download page's `xattr -dr com.apple.quarantine`. Screenshots land in $OUT/shots (default
# out/mac-install), logs in $OUT/logs; a failure exits 1 after every app has run.
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
dist=$(cd "${1:?usage: check_mac_install.sh DIST [APPS]}" && pwd)
apps=${2:-brick slate frame etch}
out=${OUT:-out/mac-install}
mkdir -p "$out/shots" "$out/logs" "$out/files" "$out/site"
out=$(cd "$out" && pwd)
shots=$out/shots logs=$out/logs files=$out/files site=$out/site
bin=$out
# shellcheck source=scripts/mac_helpers.sh
source "$root/scripts/mac_helpers.sh"
unset GSK_RENDERER # the apps choose cairo themselves on macOS (suite_ui::renderer)
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

# The apps Launch Services offers for a file (Finder's Open With).
cat >"$out/openers.swift" <<'EOF'
import AppKit
let url = URL(fileURLWithPath: CommandLine.arguments[1])
for app in NSWorkspace.shared.urlsForApplications(toOpen: url) { print(app.path) }
EOF
swiftc -O "$out/openers.swift" -o "$out/openers" >>"$logs/swiftc.log" 2>&1 || { note "FAIL openers didn't build"; exit 1; }

if [[ -e /opt/homebrew/lib ]]; then
  fail "Homebrew is at /opt/homebrew: this check needs a Mac without it"
else
  pass "no Homebrew at /opt/homebrew ($(sw_vers -productVersion), $(uname -m))"
fi
note "Gatekeeper: $(spctl --status 2>&1)"

# The sample files.
printf 'id,name,city\n1,Ada,Paris\n2,Linus,Austin\n' >"$files/people.csv"
cp "$root/apps/brick/crates/xlsx/tests/data/fixture.xlsx" "$files/book.xlsx"
printf '# Read me\n\nInstalled by **curl**.\n' >"$files/readme.md"
cp "$root/apps/slate/crates/docx/tests/data/fixture.docx" "$files/letter.docx"
cp "$root/apps/frame/crates/pptx/tests/data/fixture.pptx" "$files/deck.pptx"
cp "$root/apps/frame/crates/pptx/tests/data/shapes.pptx" "$files/shapes.pptx"
wide_png "$files/wide.png"
cp "$root/apps/etch/crates/image-io/tests/data/alpha.png" "$files/alpha.png"

# The download site: per app a good and a tampered release.
port=$((20000 + RANDOM % 20000))
for app in $apps; do
  tar=$(ls "$dist/$app"-*-arm64-macos.tar.gz 2>/dev/null | head -1)
  [[ -n $tar ]] || { fail "$app: no macOS tarball in $dist"; continue; }
  name=$(basename "$tar")
  version=${name#"$app"-}
  version=${version%-arm64-macos.tar.gz}
  sha=$(shasum -a 256 "$tar" | cut -d' ' -f1)
  for kind in good bad; do
    mkdir -p "$site/$kind/$app/releases"
    cp "$tar" "$site/$kind/$app/releases/"
    printf '{"platforms":{"macos-arm64":{"url":"http://127.0.0.1:%s/%s/%s/releases/%s","sha256":"%s","bytes":%s}},"version":"%s","date":"2026-10-15","url":"http://127.0.0.1:%s/none.tar.gz","sha256":"0","bytes":0}\n' \
      "$port" "$kind" "$app" "$name" "$sha" "$(stat -f %z "$tar")" "$version" "$port" >"$site/$kind/$app/latest.json"
  done
  printf 'x' >>"$site/bad/$app/releases/$name"
  cp "$root/sites/$app/install.sh" "$site/$app-install.sh"
done
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$site" >"$logs/http.log" 2>&1 &
server=$!
trap 'kill $server 2>/dev/null' EXIT
sleep 1

# Run app $1's installer, as the download page says, against release $2 (good or bad),
# with arguments $3...; output in $logs/$1-install-$2$4.log.
installer() {
  local app=$1 kind=$2 up
  shift 2
  up=$(tr '[:lower:]' '[:upper:]' <<<"$app")
  curl -fsSL "http://127.0.0.1:$port/$app-install.sh" |
    env "${up}_DOWNLOAD_URL=http://127.0.0.1:$port/$kind/$app" sh -s -- "$@"
}

# The document windows (layer 0) of process $1, after up to 20 s for $2 of them.
wait_windows() {
  local i
  for i in $(seq 40); do
    (($(windows "$1" | awk -F'\t' '$1 == 0' | wc -l) >= $2)) && break
    sleep 0.5
  done
  sleep 2
  windows "$1" | awk -F'\t' '$1 == 0'
}

# The pids running bundle $1's executable.
pids() { pgrep -f "^$1/Contents/MacOS/" || true; }

# Check app $1 (bundle name $2) with sample files $3 and $4.
check() {
  local app=$1 name=$2 one=$3 two=$4 appdir exe id pid list dmg mnt
  appdir=$HOME/Applications/$name.app exe=$HOME/Applications/$name.app/Contents/MacOS/$app id=dev.cronkite.$name

  # 1. A tampered download.
  if installer "$app" bad >"$logs/$app-install-bad.log" 2>&1; then
    fail "$app: the installer took a tampered tarball"
  elif grep -q "checksum mismatch" "$logs/$app-install-bad.log" && [[ ! -e $appdir ]]; then
    pass "$app: a tampered tarball is refused, nothing installed"
  else fail "$app: tampered tarball: $(tr '\n' '|' <"$logs/$app-install-bad.log")"; fi

  # 2. The installer.
  if installer "$app" good >"$logs/$app-install.log" 2>&1 && grep -q "^Installed $name " "$logs/$app-install.log"; then
    pass "$app: installed: $(head -2 "$logs/$app-install.log" | tail -1)"
  else fail "$app: install: $(tr '\n' '|' <"$logs/$app-install.log")"; return; fi
  [[ -x $exe ]] && pass "$app: $appdir" || fail "$app: no $exe"
  if xattr -r "$appdir" 2>/dev/null | grep -q com.apple.quarantine; then
    fail "$app: the installed app is quarantined"
  else pass "$app: no quarantine flag on the installed app ($(xattr "$appdir" | tr '\n' ' '))"; fi
  if codesign --verify --deep --strict "$appdir" 2>"$logs/$app-codesign.log"; then
    pass "$app: signature valid ($(codesign -dv "$appdir" 2>&1 | grep -E '^Signature' | tr '\n' ' '))"
  else fail "$app: codesign --verify: $(tr '\n' '|' <"$logs/$app-codesign.log")"; fi
  note "$app: spctl --assess: $(spctl --assess --type execute "$appdir" 2>&1 | tr '\n' ' ')"
  if "$lsregister" -dump 2>/dev/null | grep -q "path: *$appdir "; then
    pass "$app: registered with Launch Services"
  elif "$lsregister" -dump 2>/dev/null | grep -qF "$appdir"; then
    pass "$app: registered with Launch Services"
  else fail "$app: not in Launch Services' database"; fi
  "$out/openers" "$one" >"$logs/$app-openers.txt" 2>&1
  if grep -qxF "$appdir" "$logs/$app-openers.txt"; then
    pass "$app: Open With lists it for $(basename "$one")"
  else fail "$app: Open With for $(basename "$one"): $(tr '\n' '|' <"$logs/$app-openers.txt")"; fi

  # 3. Start it, logging where every library comes from.
  DYLD_PRINT_LIBRARIES=1 "$exe" "$one" >"$logs/$app-start.log" 2>&1 &
  pid=$!
  list=$(wait_windows "$pid" 1)
  echo "$list" >"$logs/$app-start-windows.txt"
  if kill -0 "$pid" 2>/dev/null && [[ -n $list ]]; then
    pass "$app: started on $(basename "$one"): $(tr '\n' '|' <<<"$list")"
  else fail "$app: no window: $(grep -v '^dyld' "$logs/$app-start.log" | tail -5 | tr '\n' '|')"; fi
  front "$pid"
  shot "$app-1-installed"
  local homebrew bundled
  homebrew=$(grep -c '/opt/homebrew' "$logs/$app-start.log")
  bundled=$(grep -cF "$appdir/Contents/Frameworks/" "$logs/$app-start.log")
  if ((homebrew == 0 && bundled > 10)); then
    pass "$app: $bundled libraries from the bundle, none from /opt/homebrew"
  else fail "$app: libraries: $bundled from the bundle, $homebrew from /opt/homebrew"; fi
  grep -q "running from the bundle $appdir/Contents/Resources" "$logs/$app-start.log" &&
    pass "$app: runs from the bundle" || fail "$app: didn't set up the bundle: $(grep -i bundle "$logs/$app-start.log" | head -3)"
  if [[ $app != etch ]]; then
    grep -q "fonts: added $appdir/Contents/Resources/fonts" "$logs/$app-start.log" &&
      pass "$app: the bundled fonts are in" || fail "$app: the bundled fonts weren't added"
  fi
  [[ -s $HOME/Library/Caches/$id/gdk-pixbuf-loaders.cache ]] &&
    pass "$app: gdk-pixbuf's loader list: $(grep -c '\.so"$' "$HOME/Library/Caches/$id/gdk-pixbuf-loaders.cache") loaders in the bundle" ||
    fail "$app: no gdk-pixbuf loader list in ~/Library/Caches/$id"
  [[ -d $HOME/Library/Caches/$id/fontconfig ]] &&
    pass "$app: fontconfig's cache is in ~/Library/Caches/$id (the bundle's fonts.conf)" ||
    fail "$app: no fontconfig cache in ~/Library/Caches/$id"
  # About (the app menu's first item): the dialog with the app's icon.
  osa "tell application \"System Events\" to tell (first process whose unix id is $pid) to click menu item 1 of menu 1 of menu bar item 2 of menu bar 1" || true
  shot "$app-2-about" 2
  stop
  if [[ $app == frame ]]; then
    [[ -d $HOME/Library/Caches/$id ]] && pass "$app: ~/Library/Caches/$id (Frame keeps no settings or state)" ||
      fail "$app: no ~/Library/Caches/$id"
  else
    [[ -d "$HOME/Library/Application Support/$id" ]] &&
      pass "$app: ~/Library/Application Support/$id: $(ls "$HOME/Library/Application Support/$id" | tr '\n' ' ')" ||
      fail "$app: no ~/Library/Application Support/$id"
  fi

  # 4. Through Launch Services.
  open -a "$appdir" "$two"
  sleep 1
  pid=$(pids "$appdir" | head -1)
  if [[ -n $pid ]]; then
    list=$(wait_windows "$pid" 1)
    shot "$app-3-open-a"
    grep -qF "$(basename "$two")" <<<"$list" &&
      pass "$app: open -a $(basename "$two") showed it" || fail "$app: open -a $(basename "$two"): $(tr '\n' '|' <<<"$list")"
    kill "$pid"; sleep 2; kill -9 "$pid" 2>/dev/null
  else fail "$app: open -a started nothing"; fi

  # 5. Uninstall.
  if installer "$app" good --uninstall >"$logs/$app-uninstall.log" 2>&1 && [[ ! -e $appdir ]] &&
    ! "$lsregister" -dump 2>/dev/null | grep -qF "$appdir"; then
    pass "$app: --uninstall: $(cat "$logs/$app-uninstall.log")"
  else fail "$app: --uninstall left $(ls -d "$appdir" 2>/dev/null) $(tr '\n' '|' <"$logs/$app-uninstall.log")"; fi

  # 6. The DMG.
  dmg=$(ls "$dist/$app"-*-arm64-macos.dmg 2>/dev/null | head -1)
  [[ -n $dmg ]] || { fail "$app: no DMG in $dist"; return; }
  mnt=$out/mnt-$app
  mkdir -p "$mnt"
  if ! hdiutil attach -nobrowse -readonly -mountpoint "$mnt" "$dmg" >"$logs/$app-attach.log" 2>&1; then
    fail "$app: the DMG doesn't mount: $(tr '\n' '|' <"$logs/$app-attach.log")"; return
  fi
  [[ -d $mnt/$name.app && -L $mnt/Applications && $(readlink "$mnt/Applications") == /Applications ]] &&
    pass "$app: the DMG holds $name.app and an Applications link ($(ls "$mnt" | tr '\n' ' '))" ||
    fail "$app: the DMG holds $(ls -l "$mnt" | tr '\n' '|')"
  rm -rf "/Applications/$name.app"
  ditto "$mnt/$name.app" "/Applications/$name.app"
  hdiutil detach "$mnt" >/dev/null 2>&1 || hdiutil detach -force "$mnt" >/dev/null 2>&1
  "$lsregister" -f "/Applications/$name.app"
  open -a "/Applications/$name.app" "$one"
  sleep 1
  pid=$(pids "/Applications/$name.app" | head -1)
  if [[ -n $pid ]]; then
    list=$(wait_windows "$pid" 1)
    shot "$app-4-from-dmg"
    grep -qF "$(basename "$one")" <<<"$list" &&
      pass "$app: from the DMG, /Applications/$name.app opened $(basename "$one")" ||
      fail "$app: from the DMG: $(tr '\n' '|' <<<"$list")"
    kill "$pid"; sleep 2; kill -9 "$pid" 2>/dev/null
  else fail "$app: /Applications/$name.app from the DMG started nothing"; fi
  "$lsregister" -u "/Applications/$name.app" 2>/dev/null
  rm -rf "/Applications/$name.app"
}

# The no-file windows of Frame and Etch are "Untitled"; with a file, its name.
for app in $apps; do
  case $app in
    brick) check brick Brick "$files/people.csv" "$files/book.xlsx" ;;
    slate) check slate Slate "$files/readme.md" "$files/letter.docx" ;;
    frame) check frame Frame "$files/deck.pptx" "$files/shapes.pptx" ;;
    etch) check etch Etch "$files/wide.png" "$files/alpha.png" ;;
  esac
done

# Gatekeeper and the download page's instructions: a DMG from a browser is quarantined.
gk_app=$(for a in $apps; do echo "$a"; done | tail -1)
gk_name=$(tr '[:lower:]' '[:upper:]' <<<"${gk_app:0:1}")${gk_app:1}
gk_dmg=$(ls "$dist/$gk_app"-*-arm64-macos.dmg 2>/dev/null | head -1)
if [[ -n $gk_dmg ]]; then
  gk=/Applications/$gk_name.app
  mkdir -p "$out/mnt-gk"
  hdiutil attach -nobrowse -readonly -mountpoint "$out/mnt-gk" "$gk_dmg" >/dev/null 2>&1
  rm -rf "$gk"
  ditto "$out/mnt-gk/$gk_name.app" "$gk"
  hdiutil detach "$out/mnt-gk" >/dev/null 2>&1 || hdiutil detach -force "$out/mnt-gk" >/dev/null 2>&1
  xattr -w com.apple.quarantine "0083;$(printf %x "$(date +%s)");Safari;$(uuidgen)" "$gk"
  note "$gk_app: quarantined like a browser download: $(xattr -p com.apple.quarantine "$gk"); spctl: $(spctl --assess --type execute "$gk" 2>&1 | tr '\n' ' ')"
  open -a "$gk" >"$logs/gatekeeper-open.log" 2>&1
  sleep 8
  shot gatekeeper-1-quarantined
  pid=$(pids "$gk" | head -1)
  note "$gk_app: opening it quarantined: $([[ -n $pid ]] && echo "it started (pid $pid; Gatekeeper let it)" || echo "it didn't start (Gatekeeper)") $(tr '\n' ' ' <"$logs/gatekeeper-open.log")"
  [[ -n $pid ]] && { kill "$pid"; sleep 2; kill -9 "$pid" 2>/dev/null; }
  killall CoreServicesUIAgent 2>/dev/null
  xattr -dr com.apple.quarantine "$gk"
  open -a "$gk" "$files/wide.png"
  sleep 1
  pid=$(pids "$gk" | head -1)
  if [[ -n $pid ]] && [[ -n $(wait_windows "$pid" 1) ]]; then
    pass "$gk_app: after xattr -dr com.apple.quarantine it starts"
  else fail "$gk_app: after xattr -dr com.apple.quarantine it doesn't start"; fi
  shot gatekeeper-2-after-xattr
  [[ -n $pid ]] && { kill "$pid"; sleep 2; kill -9 "$pid" 2>/dev/null; }
  "$lsregister" -u "$gk" 2>/dev/null
  rm -rf "$gk"
fi

if ((failed)); then note "check_mac_install: FAILED"; exit 1; fi
note "check_mac_install: OK"
