import Foundation

/// Exercises the real Swift and Rust sealers, without capture, sockets or app state.
@main
enum MediaCryptoInteropTests {
    static func decodeHex(_ value: String) -> Data {
        let bytes = Array(value.utf8)
        precondition(bytes.count.isMultiple(of: 2))
        return Data(stride(from: 0, to: bytes.count, by: 2).map {
            UInt8(String(decoding: bytes[$0..<$0 + 2], as: UTF8.self), radix: 16)!
        })
    }

    static func hex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }

    static func rust(_ executable: String, _ args: [String]) throws -> (Int32, String) {
        let process = Process()
        let output = Pipe()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = args
        process.standardOutput = output
        try process.run()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, String(decoding: data, as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines))
    }

    static func main() throws {
        precondition(CommandLine.arguments.count == 3, "expected Rust peer and shared fixture paths")
        let peer = CommandLine.arguments[1]
        let fixture = try String(contentsOfFile: CommandLine.arguments[2], encoding: .utf8)
        let fields = Dictionary(uniqueKeysWithValues: fixture.split(separator: "\n")
            .filter { !$0.hasPrefix("#") }.map { line in
                let pair = line.split(separator: "=", maxSplits: 1)
                return (String(pair[0]), decodeHex(String(pair[1])))
            })
        let key = fields["media_key_hex"]!
        let plaintext = fields["plaintext_hex"]!
        let keys = MediaKeyDerivation.directionalKeys(mediaKey: key)!
        var failures = 0
        func check(_ passed: Bool, _ name: String) {
            print("\(passed ? "PASS" : "FAIL"): \(name)")
            if !passed { failures += 1 }
        }
        for (direction, directionalKey) in [("c2s", keys.c2s), ("s2c", keys.s2c)] {
            let frame = fields["\(direction)_frame_hex"]!
            var receiver = MediaSealer(mediaKey: directionalKey)!
            check(receiver.open(frame) == plaintext, "shared \(direction) ciphertext/tag vector")
            check(receiver.open(frame) == nil, "\(direction) replay rejected")
            var forged = frame
            forged[forged.count - 1] ^= 1
            var fresh = MediaSealer(mediaKey: directionalKey)!
            check(fresh.open(forged) == nil, "\(direction) forged tag rejected")
            check(fresh.open(frame.prefix(23)) == nil, "\(direction) truncated frame rejected")
            check(fresh.open(frame) == plaintext, "\(direction) forgery does not consume counter")
        }
        let host = MediaSessionCrypto(mediaKey: key)!
        let sealed = host.seal(plaintext)!
        let openedByRust = try rust(peer, ["open-s2c", hex(sealed)])
        check(openedByRust.0 == 0 && openedByRust.1 == hex(plaintext),
              "actual Swift host seal -> Rust viewer open")
        let sealedByRust = try rust(peer, ["seal-c2s"])
        check(sealedByRust.0 == 0 && host.open(decodeHex(sealedByRust.1)) == plaintext,
              "actual Rust viewer seal -> Swift host open")
        check(host.open(decodeHex(sealedByRust.1)) == nil, "actual Rust reply replay rejected")
        check(host.open(sealed) == nil, "host rejects its own opposite-direction frame")
        if failures > 0 {
            print("\(failures) media interoperability checks failed")
            exit(1)
        }
        print("MediaCryptoInteropTests passed: shared vectors and actual bidirectional crypto")
    }
}
