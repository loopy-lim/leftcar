// Host-side launcher for the macOS capture shim (v2 handle-based ABI).
//
// Loads libleftcar_capture.dylib (Swift SCK+VT shim), starts a capture
// session toward the viewer's TCP endpoint, prints live stats JSON.
//
// Usage:
//   swift tools/capture_host.swift --list
//   swift tools/capture_host.swift <viewer_ip> <port> [secs] [--display N] [--size WxH] [--fps F]

import Foundation

let args = CommandLine.arguments

if args.contains("--list") {
    let dylibPath = "native/macos-capture-shim/libleftcar_capture.dylib"
    guard let handle = dlopen(dylibPath, RTLD_NOW) else { exit(1) }
    typealias ListDisplays = @convention(c) () -> UnsafeMutablePointer<CChar>
    typealias FreeString = @convention(c) (UnsafeMutablePointer<CChar>) -> Void
    guard
        let list = dlsym(handle, "leftcar_capture_list_displays").map({ unsafeBitCast($0, to: ListDisplays.self) }),
        let freeStr = dlsym(handle, "leftcar_capture_free_string").map({ unsafeBitCast($0, to: FreeString.self) })
    else { exit(1) }
    let raw = list()
    print(String(cString: raw))
    freeStr(raw)
    exit(0)
}

guard args.count >= 3 else {
    FileHandle.standardError.write("usage: capture_host.swift <viewer_ip> <port> [secs] [--display N] [--size WxH] [--fps F] | --list\n".data(using: .utf8)!)
    exit(2)
}
let host = args[1]
let port = UInt16(args[2]) ?? 5000
var seconds = 60.0
var display: UInt32 = 0
var width: UInt32 = 1920
var height: UInt32 = 1080
var fps: UInt32 = 90

var i = 3
while i < args.count {
    switch args[i] {
    case "--display": display = UInt32(args[i + 1]) ?? 0; i += 2
    case "--size":
        let parts = args[i + 1].split(separator: "x").compactMap { UInt32($0) }
        if parts.count == 2 { width = parts[0]; height = parts[1] }
        i += 2
    case "--fps": fps = UInt32(args[i + 1]) ?? 90; i += 2
    default: seconds = Double(args[i]) ?? 60.0; i += 1
    }
}

let dylibPath = "native/macos-capture-shim/libleftcar_capture.dylib"

guard let handle = dlopen(dylibPath, RTLD_NOW) else {
    if let err = dlerror() {
        FileHandle.standardError.write("dlopen failed: \(String(cString: err))\n".data(using: .utf8)!)
    }
    exit(1)
}
print("loaded \(dylibPath)")

typealias StartV2 = @convention(c) (UnsafePointer<CChar>, UInt16, UInt32, UInt32, UInt32, UInt32) -> UInt32
typealias StopV2 = @convention(c) (UInt32) -> Int32
typealias StatsV2 = @convention(c) (UInt32) -> UnsafeMutablePointer<CChar>
typealias FreeString = @convention(c) (UnsafeMutablePointer<CChar>) -> Void
typealias LastError = @convention(c) () -> UnsafePointer<CChar>

guard
    let start = dlsym(handle, "leftcar_capture_start_v2").map({ unsafeBitCast($0, to: StartV2.self) }),
    let stop = dlsym(handle, "leftcar_capture_stop_v2").map({ unsafeBitCast($0, to: StopV2.self) }),
    let stats = dlsym(handle, "leftcar_capture_stats_v2").map({ unsafeBitCast($0, to: StatsV2.self) }),
    let freeStr = dlsym(handle, "leftcar_capture_free_string").map({ unsafeBitCast($0, to: FreeString.self) }),
    let lastError = dlsym(handle, "leftcar_capture_last_error_v2").map({ unsafeBitCast($0, to: LastError.self) })
else {
    FileHandle.standardError.write("missing v2 C ABI symbols in shim\n".data(using: .utf8)!)
    exit(1)
}

print("target \(host):\(port) display=\(display) size=\(width)x\(height) fps=\(fps) — starting (needs Screen Recording permission)...")
let h = start(host, port, display, width, height, fps)
if h == 0 {
    FileHandle.standardError.write("capture start FAILED: \(String(cString: lastError()))\n".data(using: .utf8)!)
    exit(1)
}

let t0 = Date()
while Date().timeIntervalSince(t0) < seconds {
    Thread.sleep(forTimeInterval: 2.0)
    let raw = stats(h)
    print(String(format: "t=%4.0fs %@", Date().timeIntervalSince(t0), String(cString: raw)))
    freeStr(raw)
}

print("stopping…")
_ = stop(h)
dlclose(handle)
print("done")
