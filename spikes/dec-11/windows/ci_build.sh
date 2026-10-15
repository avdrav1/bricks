#!/usr/bin/env bash
# DEC-11 spike (throwaway): check the workspace, test each package, release-build each app,
# and write logs plus a one-line-per-step summary into OUT (default: logs). Never stops
# at a failure: the point is to see every failure.
# MODE: all (default) or tests (skip check and the release build).
set -u
out=${1:-logs}
mode=${2:-all}
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

if [ "$mode" = all ]; then
    SECONDS=0
    cargo check --workspace --all-targets --keep-going >"$out/check.log" 2>&1
    echo "check --workspace --all-targets: exit=$? ${SECONDS}s" >>"$sum"
fi

# One package's tests into LOG; a test target that doesn't compile stops the whole run,
# so then run the unit tests and each integration test target on its own.
test_package() {
    local p=$1 log=$2 dir t
    timeout 1200 cargo test -p "$p" --no-fail-fast >"$log" 2>&1
    local rc=$?
    if grep -q '^error: could not compile' "$log"; then
        echo "== fallback: per target" >>"$log"
        timeout 1200 cargo test -p "$p" --no-fail-fast --lib >>"$log" 2>&1
        timeout 1200 cargo test -p "$p" --no-fail-fast --bins >>"$log" 2>&1
        dir=$(dirname "$(grep -l "^name = \"$p\"" Cargo.toml apps/*/Cargo.toml crates/*/Cargo.toml apps/*/crates/*/Cargo.toml | head -1)")
        for t in "$dir"/tests/*.rs; do
            [ -f "$t" ] || continue
            echo "== target $(basename "$t" .rs)" >>"$log"
            timeout 1200 cargo test -p "$p" --no-fail-fast --test "$(basename "$t" .rs)" >>"$log" 2>&1
        done
    fi
    return $rc
}


packages="suite-core suite-ui suite-platform suite-file
    csv-engine formula data-model grid commands file-format jobs xlsx brick
    document-model docx flow document-edit slate
    presentation-model pptx deck-edit frame
    image-model image-io etch"
for p in $packages; do
    SECONDS=0
    test_package "$p" "$out/test-$p.log"
    rc=$?
    broken=$(grep -oE '^error: could not compile `[^`]*` \([^)]*\)' "$out/test-$p.log" |
        sed -E 's/.*`([^`]*)` \((.*)\)/\2/' | sort -u | tr '\n' ',')
    counts=$(grep -E '^test result:' "$out/test-$p.log" |
        awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d", p, f, i}')
    failed=$(grep -E '^test .* \.\.\. FAILED$' "$out/test-$p.log" |
        sed -E 's/^test (.*) \.\.\. FAILED$/\1/' | sort -u | tr '\n' ' ')
    echo "test $p: exit=$rc $counts ${SECONDS}s ${broken:+not compiling: $broken} ${failed:+failing: $failed}" >>"$sum"
done

[ "$mode" = all ] || { cat "$sum"; exit 0; }
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
