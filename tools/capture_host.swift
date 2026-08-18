// Host-side launcher for the macOS capture shim (H16–H18 real path).
//
// Loads libleftcar_capture.dylib (Swift SCK+VT shim), points it at the
// viewer's UDP endpoint, starts capture, and prints live frame/byte stats.
// Usage: swift tools/capture_host.swift <viewer_ip> <port> [seconds]

import Foundation

let args = CommandLine.arguments
guard args.count >= 3 else {
    FileHandle.standardError.write("usage: capture_host.swift <viewer_ip> <port> [secs]\n".data(using: .utf8)!)
    exit(2)
}
let host = args[1]
let port = UInt16(args[2]) ?? 5000
let seconds = args.count > 3 ? Double(args[3])! : 60.0

let dylibPath = "native/macos-capture-shim/libleftcar_capture.dylib"

guard let handle = dlopen(dylibPath, RTLD_NOW) else {
    if let err = dlerror() {
        FileHandle.standardError.write("dlopen failed: \(String(cString: err))\n".data(using: .utf8)!)
    }
    exit(1)
}
print("loaded \(dylibPath)")

typealias SetTarget = @convention(c) (UnsafePointer<CChar>, UInt16) -> Int32
typealias Start = @convention(c) (UInt16) -> Int32
typealias Stop = @convention(c) () -> Int32
typealias Frames = @convention(c) () -> Int64
typealias Bytes = @convention(c) () -> Int64
typealias LastError = @convention(c) () -> UnsafePointer<CChar>

guard
    let setTarget = dlsym(handle, "leftcar_capture_set_target").map({ unsafeBitCast($0, to: SetTarget.self) }),
    let start = dlsym(handle, "leftcar_capture_start").map({ unsafeBitCast($0, to: Start.self) }),
    let stop = dlsym(handle, "leftcar_capture_stop").map({ unsafeBitCast($0, to: Stop.self) }),
    let frames = dlsym(handle, "leftcar_capture_frames").map({ unsafeBitCast($0, to: Frames.self) }),
    let bytes = dlsym(handle, "leftcar_capture_bytes").map({ unsafeBitCast($0, to: Bytes.self) }),
    let lastError = dlsym(handle, "leftcar_capture_last_error").map({ unsafeBitCast($0, to: LastError.self) })
else {
    FileHandle.standardError.write("missing C ABI symbols in shim\n".data(using: .utf8)!)
    exit(1)
}

_ = setTarget(host, port)
print("target \(host):\(port) — starting capture (needs Screen Recording permission)...")
let rc = start(port)
if rc != 0 {
    FileHandle.standardError.write("capture start FAILED: \(String(cString: lastError()))\n".data(using: .utf8)!)
    exit(1)
}

let t0 = Date()
var lastFrames: Int64 = 0
while Date().timeIntervalSince(t0) < seconds {
    Thread.sleep(forTimeInterval: 2.0)
    let f = frames()
    let b = bytes()
    print(String(format: "t=%4.0fs frames=%6d (+%d) bytes=%9d rate=%.1ffps",
                 Date().timeIntervalSince(t0), f, f - lastFrames, b,
                 Double(f - lastFrames) / 2.0))
    lastFrames = f
    if f == 0 && Date().timeIntervalSince(t0) > 8.0 {
        let err = String(cString: lastError())
        if !err.isEmpty {
            FileHandle.standardError.write("no frames — last error: \(err)\n".data(using: .utf8)!)
        }
    }
}

print("stopping…")
_ = stop()
dlclose(handle)
print("done")
