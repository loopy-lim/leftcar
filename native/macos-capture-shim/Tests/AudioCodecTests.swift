import Foundation
@main struct AudioCodecTests {
    static func main() throws {
        var storage = AudioPCMStorage()
        precondition(storage.convert(samples: [0, 1, -1, 0.5], channels: 2, planar: true) == [0, 0, 1, 128, 255, 127, 0, 64])
        precondition(storage.convert(samples: [.nan, .infinity], channels: 1, planar: false) == [0, 0, 0, 0])
        let first = nextAudioEncoderEpoch()!
        precondition(nextAudioEncoderEpoch()! > first)
        let encoder = try OpusAudioEncoder(channels: 2)
        let packets = try encoder.appendPCM([UInt8](repeating: 0, count: 480 * 4))
        precondition(packets.count == 1 && !packets[0].isEmpty && packets[0].count <= 1276)
        precondition(encoder.preSkip <= 5760)
        let stereo = (0..<480).flatMap { frame -> [UInt8] in
            let sample = Int16(sin(Double(frame) * 440 * 2 * Double.pi / 48000) * 12000)
            let low = UInt8(truncatingIfNeeded: sample), high = UInt8(truncatingIfNeeded: sample >> 8)
            return [low, high, low, high]
        }
        for _ in 0..<2000 { let packets = try encoder.appendPCM(stereo); precondition(packets.count == 1) }
        let replacement = try OpusAudioEncoder(channels: 2)
        precondition(replacement.epoch > encoder.epoch)
        let restarted = try replacement.appendPCM(stereo)
        precondition(restarted.count == 1)
        print("syntheticPackets=\(encoder.encodedPackets) bytes=\(encoder.encodedBytes) meanEncodeUs=\(Double(encoder.encodeNanoseconds) / Double(encoder.encodedPackets) / 1000)")
        print("AudioCodecTests passed epoch=\(encoder.epoch) preSkip=\(encoder.preSkip) bytes=\(packets[0].count)")
    }
}
