#!/bin/sh
# Cronkite installer (PKG-7). Installs or updates the whole suite, Brick, Slate, Frame, and
# Etch, for this user, without sudo, by running each app's own installer from its site
# (https://<app>.cronkite.cc/install.sh):
#
#   curl -fsSL https://cronkite.cc/install.sh | sh
#   curl -fsSL https://cronkite.cc/install.sh | sh -s -- slate frame     only the apps named
#   curl -fsSL https://cronkite.cc/install.sh | sh -s -- --uninstall      (or --uninstall slate)
#
# Linux and macOS. Each app's installer does the work and its own checks (x86_64 and GTK 4
# on Linux; Apple Silicon and macOS 14 on a Mac); this runs them one after another, keeps
# going past one that fails, and ends with each app's result. Exit status: 0 when every app
# installed (or was removed), 2 when every one installed but GTK 4 is missing (the apps'
# installers say how to get it), 1 when any failed.
#
# For tests: CRONKITE_SITE_BASE=URL takes URL/<app>/install.sh in place of each app's site,
# and the apps' installers' own variables (BRICK_DOWNLOAD_URL, SLATE_DOWNLOAD_URL, …) pass
# through to them.
set -u

ALL="brick slate frame etch"

say() { printf '%s\n' "$*"; }
die() { printf 'cronkite install: %s\n' "$*" >&2; exit 1; }

title() {
  case $1 in brick) echo Brick ;; slate) echo Slate ;; frame) echo Frame ;; etch) echo Etch ;; esac
}

fetch() { # URL FILE
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then wget -qO "$2" "$1"
  else die "needs curl or wget"; fi
}

usage() {
  say "Cronkite installer: installs or updates Brick, Slate, Frame, and Etch for this user."
  say ""
  say "  curl -fsSL https://cronkite.cc/install.sh | sh                     all four apps"
  say "  curl -fsSL https://cronkite.cc/install.sh | sh -s -- slate frame   only these"
  say "  curl -fsSL https://cronkite.cc/install.sh | sh -s -- --uninstall   remove them all"
  say ""
  say "Apps: $ALL. Each runs its own installer from its site (https://APP.cronkite.cc)."
}

# "app" or "apps", for a count.
apps() { if [ "$1" = 1 ]; then echo app; else echo apps; fi; }

# "Brick, Slate" from " Brick Slate".
listed() { echo $1 | sed 's/ /, /g'; }

main() {
  uninstall=false
  chosen=""
  for arg in "$@"; do
    case $arg in
      --uninstall) uninstall=true ;;
      -h | --help) usage; exit 0 ;;
      -*) die "unknown option $arg (use app names, --uninstall, or --help)" ;;
      *)
        app=$(printf '%s' "$arg" | tr 'A-Z' 'a-z')
        case " $ALL " in *" $app "*) ;; *) die "no app called $arg (the apps: $ALL)" ;; esac
        chosen="$chosen $app"
        ;;
    esac
  done
  # The suite's order, each once.
  want=""
  for app in $ALL; do
    if [ -z "$chosen" ]; then want="$want $app"
    else case "$chosen " in *" $app "*) want="$want $app" ;; esac; fi
  done

  case $(uname -s) in
    Linux | Darwin) ;;
    *) die "this installer is for Linux and macOS, not $(uname -s); on Windows, in PowerShell: irm https://cronkite.cc/install.ps1 | iex" ;;
  esac

  tmp=$(mktemp -d) || die "cannot make a temporary folder"
  trap 'rm -rf "$tmp"' EXIT
  trap 'exit 130' INT TERM

  if $uninstall; then verb=removed; else verb=installed; fi
  n=0; for app in $want; do n=$((n + 1)); done
  i=0; done_n=0; failed=""; nogtk=""; results=""
  for app in $want; do
    i=$((i + 1))
    name=$(title "$app")
    if [ -n "${CRONKITE_SITE_BASE:-}" ]; then site="${CRONKITE_SITE_BASE%/}/$app"; else site="https://$app.cronkite.cc"; fi
    say ""
    say "== $name ($i of $n): $site/install.sh"
    if fetch "$site/install.sh" "$tmp/$app.sh"; then
      # Its own sh, so its exit and traps stay its own; never our stdin, which may be this
      # script coming through the pipe.
      if $uninstall; then sh "$tmp/$app.sh" --uninstall </dev/null; else sh "$tmp/$app.sh" </dev/null; fi
      status=$?
    else
      printf 'cronkite install: cannot reach %s/install.sh\n' "$site" >&2
      status=1
    fi
    case $status in
      0) done_n=$((done_n + 1)); what=$verb ;;
      2) if $uninstall; then failed="$failed $name"; what="failed (see above)"
         else done_n=$((done_n + 1)); nogtk="$nogtk $name"; what="installed; needs GTK 4 to start (see above)"; fi ;;
      *) failed="$failed $name"; what="failed (see above)" ;;
    esac
    results="$results$(printf '  %-6s %s' "$name" "$what")
"
  done

  say ""
  say "Cronkite:"
  printf '%s' "$results"
  line="Cronkite: $done_n of $n $(apps "$n") $verb"
  [ -z "$failed" ] || line="$line; failed: $(listed "$failed")"
  [ -z "$nogtk" ] || line="$line; needs GTK 4: $(listed "$nogtk")"
  say "$line"
  if [ -n "$failed" ]; then exit 1; fi
  if [ -n "$nogtk" ]; then exit 2; fi
}

# Last, so sh has read the whole script before it runs any of it.
main "$@"
