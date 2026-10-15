#!/usr/bin/env bash
# DEC-11 spike (throwaway): check the workspace, test each package, release-build each app,
# and write logs plus a one-line-per-step summary into OUT (default: logs). Never stops
# at a failure: the point is to see every failure.
set -u
out=${1:-logs}
mkdir -p "$out"
sum="$out/summary.txt"
: >"$sum"

{
    cargo --version
    rustc -vV
    echo "gtk4 $(pkg-config --modversion gtk4)"
    echo "pango $(pkg-config --modversion pango)"
    echo "glib $(pkg-config --modversion glib-2.0)"
} >"$out/toolchain.txt" 2>&1

SECONDS=0
cargo check --workspace --all-targets --keep-going >"$out/check.log" 2>&1
echo "check --workspace --all-targets: exit=$? ${SECONDS}s" >>"$sum"

packages="suite-core suite-ui suite-platform suite-file
    csv-engine formula data-model grid commands file-format jobs xlsx brick
    document-model docx flow document-edit slate
    presentation-model pptx deck-edit frame
    image-model image-io etch"
for p in $packages; do
    SECONDS=0
    timeout 1200 cargo test -p "$p" --no-fail-fast >"$out/test-$p.log" 2>&1
    rc=$?
    counts=$(grep -E '^test result:' "$out/test-$p.log" |
        awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d", p, f, i}')
    failed=$(grep -E '^test .* \.\.\. FAILED$' "$out/test-$p.log" |
        sed -E 's/^test (.*) \.\.\. FAILED$/\1/' | sort -u | tr '\n' ' ')
    echo "test $p: exit=$rc $counts ${SECONDS}s ${failed:+failing: $failed}" >>"$sum"
done

SECONDS=0
cargo build --release --keep-going -p brick -p slate -p frame -p etch >"$out/build-release.log" 2>&1
echo "build --release (4 apps): exit=$? ${SECONDS}s" >>"$sum"
for app in brick slate frame etch; do
    exe=target/release/$app.exe
    if [ -f "$exe" ]; then
        echo "  $exe: $(stat -c %s "$exe") bytes" >>"$sum"
    else
        echo "  $exe: missing" >>"$sum"
    fi
done
cat "$sum"
