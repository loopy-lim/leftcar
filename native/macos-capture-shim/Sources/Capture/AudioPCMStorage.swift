import Foundation

/// Serial audio-queue conversion storage. Planar conversion writes directly
/// into the interleaved Int16 buffer without per-frame arrays.
struct AudioPCMStorage {
    var samples: [Float32] = []
    private var pcm: [UInt8] = []
    mutating func convert(samples: [Float32], channels: Int, planar: Bool) -> [UInt8] {
        guard channels > 0, samples.count % channels == 0 else { return [] }
        pcm.removeAll(keepingCapacity: true)
        pcm.reserveCapacity(samples.count * 2)
        let frames = samples.count / channels
        for frame in 0..<frames {
            for channel in 0..<channels {
                let index = planar ? channel * frames + frame : frame * channels + channel
                let value = samples[index].isFinite ? samples[index] : 0
                let sample = Int16((max(-1, min(1, value)) * 32767).rounded())
                pcm.append(UInt8(truncatingIfNeeded: sample))
                pcm.append(UInt8(truncatingIfNeeded: sample >> 8))
            }
        }
        return pcm
    }
}
