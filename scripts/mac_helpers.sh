# Shared by the macOS checks (check_mac_keys.sh, check_mac_ui.sh, check_mac_open.sh);
# source it after setting $bin (the apps' folder), $out (the output folder, absolute),
# $shots, and $logs. macOS only: GitHub's runner has a logged-in Aqua session where System
# Events may send keys and read menus, and screencapture may record.
#
# Gives: note/pass/fail (lines in $out/summary.txt; fail sets failed=1), windows PID (the
# process's windows on screen: layer, title, bounds), osa SCRIPT [OUTFILE], key, type_slowly,
# code, front PID, shot NAME [DELAY], start APP FILE (sets $pid), stop, and wide_png FILE.

export GSK_RENDERER=cairo # GL draws black windows on the runner VMs
failed=0

note() { echo "$*" | tee -a "$out/summary.txt"; }
pass() { note "ok   $*"; }
fail() { note "FAIL $*"; failed=1; }

# The windows on screen of process $1, one per line: layer, title, bounds.
cat >"$out/windows.swift" <<'EOF'
import CoreGraphics
import Foundation
let pid = Int(CommandLine.arguments[1]) ?? 0
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list where (w[kCGWindowOwnerPID as String] as? Int) == pid {
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    print("\(w[kCGWindowLayer as String] ?? 0)\t\(w[kCGWindowName as String] ?? "")\t\(b["X"] ?? 0),\(b["Y"] ?? 0) \(b["Width"] ?? 0)x\(b["Height"] ?? 0)")
}
EOF
swiftc -O "$out/windows.swift" -o "$out/windows" >"$logs/swiftc.log" 2>&1 || {
    note "FAIL the window lister didn't build"; cat "$logs/swiftc.log"; exit 1
}
windows() { "$out/windows" "$1"; }

# osascript that can't hang the job on a permission prompt; its output goes to $2 when
# given (and to the log either way).
osa() {
    local tmp p i status
    tmp=$(mktemp)
    osascript -e "$1" >"$tmp" 2>>"$logs/osascript.log" &
    p=$!
    for i in $(seq 40); do kill -0 "$p" 2>/dev/null || break; sleep 0.5; done
    if kill -0 "$p" 2>/dev/null; then kill "$p"; note "osascript timed out: $1"; rm -f "$tmp"; return 1; fi
    wait "$p"
    status=$?
    { echo "\$ $1"; cat "$tmp"; } >>"$logs/osascript.log"
    if [[ -n ${2:-} ]]; then mv "$tmp" "$2"; else rm -f "$tmp"; fi
    return $status
}
# A key with modifiers (`command down`, `{command down, shift down}`), or none.
key() { osa "tell application \"System Events\" to keystroke \"$1\"${2:+ using $2}"; sleep 0.3; }
# Characters one at a time (faster bursts garble GTK's input on the runner, spike C13).
type_slowly() {
    local s=$1 i
    for ((i = 0; i < ${#s}; i++)); do key "${s:i:1}"; done
}
code() { osa "tell application \"System Events\" to key code $1"; sleep 0.3; }
front() { osa "tell application \"System Events\" to set frontmost of (first process whose unix id is $1) to true"; sleep 1; }
shot() { sleep "${2:-1}"; screencapture -x "$shots/$1.png" || note "screencapture $1 failed"; }

# Start app $1 on file $2, drawn with GSK_RENDERER=cairo, and bring it to the front; sets
# $pid.
start() {
    "$bin/$1" "$2" >"$logs/$1.log" 2>&1 &
    pid=$!
    sleep 8
    if ! kill -0 "$pid" 2>/dev/null; then fail "$1 exited at start"; cat "$logs/$1.log"; return 1; fi
    front "$pid"
    windows "$pid" >"$logs/$1-windows-start.txt"
}
stop() {
    kill "$pid" 2>/dev/null; sleep 1; kill -9 "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
}

# A 64x32 PNG at $1: red left half, blue right.
wide_png() {
    python3 - "$1" <<'EOF'
import struct, sys, zlib
w, h = 64, 32
raw = b"".join(b"\0" + bytes([200, 40, 40]) * (w // 2) + bytes([40, 40, 200]) * (w // 2) for _ in range(h))
def chunk(t, d):
    return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
EOF
}
