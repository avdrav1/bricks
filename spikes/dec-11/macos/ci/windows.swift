// DEC-11 spike: list on-screen windows (owner, pid, layer, title, bounds) and whether this
// process may record the screen. Titles need Screen Recording permission; bounds don't.
import CoreGraphics

print("screen-capture-access\t\(CGPreflightScreenCaptureAccess())")
var count: UInt32 = 0
CGGetActiveDisplayList(0, nil, &count)
var displays = [CGDirectDisplayID](repeating: 0, count: Int(count))
CGGetActiveDisplayList(count, &displays, &count)
for d in displays {
    let mode = CGDisplayCopyDisplayMode(d)
    print("display\t\(d)\tpoints \(CGDisplayPixelsWide(d))x\(CGDisplayPixelsHigh(d))\tpixels \(mode?.pixelWidth ?? 0)x\(mode?.pixelHeight ?? 0)")
}
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list {
    let owner = w[kCGWindowOwnerName as String] as? String ?? "?"
    let pid = w[kCGWindowOwnerPID as String] as? Int ?? 0
    let name = w[kCGWindowName as String] as? String ?? ""
    let layer = w[kCGWindowLayer as String] as? Int ?? 0
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    print("window\t\(owner)\t\(pid)\t\(layer)\t\(name)\t\(b["X"] ?? 0),\(b["Y"] ?? 0) \(b["Width"] ?? 0)x\(b["Height"] ?? 0)")
}
