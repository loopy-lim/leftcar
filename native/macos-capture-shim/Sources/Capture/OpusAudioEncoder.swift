import Foundation
import AudioToolbox

private let audioEpochLock = NSLock()
private var audioEpoch: UInt64 = 0
func nextAudioEncoderEpoch() -> UInt64? {
    audioEpochLock.lock()
    defer { audioEpochLock.unlock() }
    guard audioEpoch < UInt64.max else { return nil }
    audioEpoch += 1
    return audioEpoch
}

/// Native software codec, owned exclusively by the CaptureSession audio queue.
/// Creation/property/streaming errors throw so the owner can latch PCM fallback.
final class OpusAudioEncoder {
    let channels: Int
    let epoch: UInt64
    let preSkip: UInt16
    let converter: AudioConverterRef // internal read-only handle for synthetic decoder oracle
    private let pump: OpusPacketPump
    var suppliedFrames: UInt64 { pump.input.suppliedFrames }
    var pendingPCMBytes: Int { pump.input.pendingBytes }
    private(set) var encodedBytes: UInt64 = 0
    private(set) var encodeNanoseconds: UInt64 = 0
    private(set) var encodedPackets: UInt64 = 0

    init(channels: Int) throws {
        guard (1...2).contains(channels), let epoch = nextAudioEncoderEpoch() else { throw CodecError.invalid }
        self.channels = channels
        self.epoch = epoch
        var pcm = AudioStreamBasicDescription(mSampleRate: 48000, mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
            mBytesPerPacket: UInt32(channels * 2), mFramesPerPacket: 1,
            mBytesPerFrame: UInt32(channels * 2), mChannelsPerFrame: UInt32(channels), mBitsPerChannel: 16, mReserved: 0)
        var opus = AudioStreamBasicDescription(mSampleRate: 48000, mFormatID: kAudioFormatOpus,
            mFormatFlags: 0, mBytesPerPacket: 0, mFramesPerPacket: 480,
            mBytesPerFrame: 0, mChannelsPerFrame: UInt32(channels), mBitsPerChannel: 0, mReserved: 0)
        var created: AudioConverterRef?
        try Self.check(AudioConverterNew(&pcm, &opus, &created))
        guard let created else { throw CodecError.invalid }
        do {
            var frames: UInt32 = 480
            try Self.check(AudioConverterSetProperty(created, kAudioCodecPropertyPacketFrameSize, 4, &frames))
            var bitrate: UInt32 = 128000
            try Self.check(AudioConverterSetProperty(created, kAudioConverterEncodeBitRate, 4, &bitrate))
            var actual = AudioStreamBasicDescription()
            var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
            try Self.check(AudioConverterGetProperty(created, kAudioConverterCurrentOutputStreamDescription, &size, &actual))
            guard actual.mSampleRate == 48000, actual.mFramesPerPacket == 480,
                actual.mChannelsPerFrame == UInt32(channels) else { throw CodecError.invalid }
            var maxPacket: UInt32 = 0
            size = 4
            try Self.check(AudioConverterGetProperty(created, kAudioConverterPropertyMaximumOutputPacketSize, &size, &maxPacket))
            guard maxPacket > 0, maxPacket <= 1276 else { throw CodecError.invalid }
            var prime = AudioConverterPrimeInfo()
            size = UInt32(MemoryLayout<AudioConverterPrimeInfo>.size)
            try Self.check(AudioConverterGetProperty(created, kAudioConverterPrimeInfo, &size, &prime))
            guard prime.leadingFrames <= 5760 else { throw CodecError.invalid }
            preSkip = UInt16(prime.leadingFrames)
            pump = OpusPacketPump(channels: channels, maximumPacket: Int(maxPacket))
            converter = created
        } catch {
            AudioConverterDispose(created)
            throw error
        }
    }
    deinit { AudioConverterDispose(converter) }
    enum CodecError: Error { case status(OSStatus), invalid }
    private static func check(_ result: OSStatus) throws {
        if result != noErr { throw CodecError.status(result) }
    }
    func appendPCM(_ pcm: [UInt8]) throws -> [Data] {
        let packets = try pump.appendPCM(pcm) { input, produced, list, description in
            let began = DispatchTime.now().uptimeNanoseconds
            let status = AudioConverterFillComplexBuffer(converter, opusPCMInput,
                Unmanaged.passUnretained(input).toOpaque(), &produced, &list, &description)
            encodeNanoseconds += DispatchTime.now().uptimeNanoseconds - began
            return status
        }
        encodedBytes += packets.reduce(0) { $0 + UInt64($1.count) }
        encodedPackets += UInt64(packets.count)
        return packets
    }
}
