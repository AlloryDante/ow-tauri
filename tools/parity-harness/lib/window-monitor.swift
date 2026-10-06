// Window monitor (macOS): records every window that a process owns, as the
// window server sees it, so a run can prove that none of them became visible.
//
//   window-monitor <pid> <out.jsonl> [intervalMs]
//
// Polls CGWindowListCopyWindowInfo. Writes one JSON line whenever the set of
// the process's windows (id, layer, alpha, bounds, on-screen flag) changes,
// plus a "visible" flag: on screen, alpha > 0.01, and intersecting a display.
// Exits when the process is gone. Needs no screen-recording permission
// (window names are not read).

import CoreGraphics
import Foundation

let args = CommandLine.arguments
guard args.count >= 3, let pid = Int32(args[1]) else {
  FileHandle.standardError.write("usage: window-monitor <pid> <out.jsonl> [intervalMs]\n".data(using: .utf8)!)
  exit(2)
}
let outPath = args[2]
let intervalMs = args.count > 3 ? (Int(args[3]) ?? 25) : 25
FileManager.default.createFile(atPath: outPath, contents: nil)
guard let out = FileHandle(forWritingAtPath: outPath) else { exit(1) }

func displayBounds() -> [CGRect] {
  var count: UInt32 = 0
  CGGetActiveDisplayList(0, nil, &count)
  var ids = [CGDirectDisplayID](repeating: 0, count: Int(count))
  CGGetActiveDisplayList(count, &ids, &count)
  return ids.map { CGDisplayBounds($0) }
}

let start = Date()
var last = ""
var everVisible = false
var samples = 0

func write(_ object: [String: Any]) {
  if let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) {
    out.write(data)
    out.write("\n".data(using: .utf8)!)
  }
}

write(["kind": "start", "pid": Int(pid), "intervalMs": intervalMs, "displays": displayBounds().map { ["x": $0.origin.x, "y": $0.origin.y, "w": $0.size.width, "h": $0.size.height] }])

while kill(pid, 0) == 0 {
  samples += 1
  let displays = displayBounds()
  let list = (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]]) ?? []
  var windows: [[String: Any]] = []
  var anyVisible = false
  for info in list {
    guard let owner = info[kCGWindowOwnerPID as String] as? Int32, owner == pid else { continue }
    let alpha = info[kCGWindowAlpha as String] as? Double ?? -1
    let onscreen = info[kCGWindowIsOnscreen as String] as? Bool ?? false
    var rect = CGRect.zero
    if let b = info[kCGWindowBounds as String] as? NSDictionary {
      CGRectMakeWithDictionaryRepresentation(b as CFDictionary, &rect)
    }
    let onDisplay = displays.contains { $0.intersects(rect) }
    let visible = onscreen && alpha > 0.01 && onDisplay && rect.width > 0 && rect.height > 0
    if visible { anyVisible = true }
    windows.append([
      "id": info[kCGWindowNumber as String] as? Int ?? -1,
      "layer": info[kCGWindowLayer as String] as? Int ?? 0,
      "alpha": alpha,
      "onscreen": onscreen,
      "onDisplay": onDisplay,
      "visible": visible,
      "x": rect.origin.x, "y": rect.origin.y, "w": rect.size.width, "h": rect.size.height,
    ])
  }
  if anyVisible { everVisible = true }
  let key = windows.map { "\($0["id"]!)|\($0["alpha"]!)|\($0["onscreen"]!)|\($0["x"]!),\($0["y"]!),\($0["w"]!),\($0["h"]!)" }.joined(separator: ";")
  if key != last {
    last = key
    write(["kind": "change", "ms": Int(Date().timeIntervalSince(start) * 1000), "anyVisible": anyVisible, "windows": windows])
  }
  usleep(useconds_t(intervalMs * 1000))
}
write(["kind": "end", "ms": Int(Date().timeIntervalSince(start) * 1000), "samples": samples, "everVisible": everVisible])
