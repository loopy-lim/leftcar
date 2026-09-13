import Foundation
import AudioToolbox

private final class PacketOracleInput {
    let packets: [Data]
    var index = 0
    let loan = UnsafeMutableRawPointer.allocate(byteCount: 1276, alignment: 2)
    let description = UnsafeMutablePointer<AudioStreamPacketDescription>.allocate(capacity: 1)
    init(_ packets: [Data]) { self.packets = packets; description.initialize(to: AudioStreamPacketDescription()) }
    deinit { loan.deallocate(); description.deinitialize(count: 1); description.deallocate() }
}
private let oracleInput: AudioConverterComplexInputDataProc = { _, count, list, descriptions, context in
    let input = Unmanaged<PacketOracleInput>.fromOpaque(context!).takeUnretainedValue()
    guard input.index < input.packets.count else {
        count.pointee = 0; list.pointee.mBuffers.mData = nil; list.pointee.mBuffers.mDataByteSize = 0
        return noErr // explicit finite test EOF, unlike the continuous encoder input
    }
    let packet = input.packets[input.index]; input.index += 1
    packet.withUnsafeBytes { input.loan.copyMemory(from: $0.baseAddress!, byteCount: packet.count) }
    input.description.pointee = AudioStreamPacketDescription(mStartOffset: 0, mVariableFramesInPacket: 480, mDataByteSize: UInt32(packet.count))
    descriptions?.pointee = input.description
    count.pointee = 1
    list.pointee.mNumberBuffers = 1
    list.pointee.mBuffers = AudioBuffer(mNumberChannels: 2, mDataByteSize: UInt32(packet.count), mData: input.loan)
    return noErr
}
private func decode(_ packets: [Data], encoder: OpusAudioEncoder) -> [Int16] {
    var compressed = AudioStreamBasicDescription()
    var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
    precondition(AudioConverterGetProperty(encoder.converter, kAudioConverterCurrentOutputStreamDescription, &size, &compressed) == noErr)
    var pcm = AudioStreamBasicDescription(mSampleRate: 48000, mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
        mBytesPerPacket: 4, mFramesPerPacket: 1, mBytesPerFrame: 4, mChannelsPerFrame: 2, mBitsPerChannel: 16, mReserved: 0)
    var decoder: AudioConverterRef?
    precondition(AudioConverterNew(&compressed, &pcm, &decoder) == noErr)
    let active = decoder!
    defer { AudioConverterDispose(active) }
    var writable = DarwinBoolean(false)
    precondition(AudioConverterGetPropertyInfo(encoder.converter, kAudioConverterCompressionMagicCookie, &size, &writable) == noErr)
    var cookie = [UInt8](repeating: 0, count: Int(size))
    cookie.withUnsafeMutableBytes { bytes in
        precondition(AudioConverterGetProperty(encoder.converter, kAudioConverterCompressionMagicCookie, &size, bytes.baseAddress!) == noErr)
        precondition(AudioConverterSetProperty(active, kAudioConverterDecompressionMagicCookie, size, bytes.baseAddress!) == noErr)
    }
    let input = PacketOracleInput(packets)
    var output = [Int16](repeating: 0, count: (packets.count * 480 + 960) * 2)
    var count = UInt32(output.count / 2)
    let status = output.withUnsafeMutableBytes { bytes in
        var list = AudioBufferList(mNumberBuffers: 1,
            mBuffers: AudioBuffer(mNumberChannels: 2, mDataByteSize: UInt32(bytes.count), mData: bytes.baseAddress))
        return AudioConverterFillComplexBuffer(active, oracleInput, Unmanaged.passUnretained(input).toOpaque(), &count, &list, nil)
    }
    precondition(status == noErr && input.index == packets.count)
    var drainFrames: UInt32 = 480
    var drain = [Int16](repeating: 0, count: 960)
    drain.withUnsafeMutableBytes { bytes in
        var list = AudioBufferList(mNumberBuffers: 1, mBuffers: AudioBuffer(mNumberChannels: 2, mDataByteSize: UInt32(bytes.count), mData: bytes.baseAddress))
        precondition(AudioConverterFillComplexBuffer(active, oracleInput, Unmanaged.passUnretained(input).toOpaque(), &drainFrames, &list, nil) == noErr)
    }
    print("oracle finiteEOF drainFrames=\(drainFrames)")
    precondition(drainFrames == 0)
    print("oracle inputPackets=\(input.index) decodedFrames=\(count) codedFrames=\(packets.count * 480)")
    return Array(output.prefix(Int(count) * 2))
}
@main struct OpusContinuityTests {
    static func main() throws {
        setbuf(stdout, nil)
        // An isolated known input impulse establishes the decoder's leading
        // boundary independently of any total-frame deficit or waveform fit.
        let markerEncoder = try OpusAudioEncoder(channels: 2)
        var marker = [Int16](repeating: 0, count: 4800 * 2)
        marker[960 * 2] = 28000; marker[960 * 2 + 1] = 28000
        let markerPackets = try markerEncoder.appendPCM(marker.withUnsafeBytes { Array($0) })
        let markerDecoded = decode(markerPackets, encoder: markerEncoder)
        let peak = (0..<markerDecoded.count / 2).max { abs(Int(markerDecoded[$0 * 2])) < abs(Int(markerDecoded[$1 * 2])) }!
        print("knownImpulse inputFrame=960 decodedPeak=\(peak) leadingAlignment=\(peak - 960)")
        precondition(peak - 960 == 192)
        let frames = 144000
        var source: [Int16] = []
        for frame in 0..<frames {
            let t = Double(frame) / 48000
            for channel in 0..<2 {
                let phase = 2 * Double.pi * ((channel == 0 ? 1000.0 : 1600.0) * t + 131 * t * t)
                source.append(Int16(sin(phase) * (9000 + 2200 * sin(t * 3.1))))
            }
        }
        let pcm = source.withUnsafeBytes { Array($0) }
        let encoder = try OpusAudioEncoder(channels: 2)
        var packets: [Data] = []
        let fragments = [1, 7, 130, 1919, 2501, 19, 8000]
        var offset = 0, turn = 0
        while offset < pcm.count {
            let end = min(pcm.count, offset + fragments[turn % fragments.count])
            packets += try encoder.appendPCM(Array(pcm[offset..<end]))
            offset = end; turn += 1
        }
        packets += try encoder.appendPCM([]) // temporary shortage, never encoder EOF
        precondition(encoder.suppliedFrames == UInt64(frames) && encoder.pendingPCMBytes == 0)
        let regular = try OpusAudioEncoder(channels: 2)
        var regularPackets: [Data] = []
        for start in stride(from: 0, to: pcm.count, by: 1920) { regularPackets += try regular.appendPCM(Array(pcm[start..<min(start + 1920, pcm.count)])) }
        precondition(packets == regularPackets, "fragmentation must not change any encoded packet")
        print("fragmentedMatchesRegularPackets=true")
        let decoded = decode(packets, encoder: encoder)
        precondition(decoded.count / 2 >= frames - 960 && decoded.count / 2 <= frames + 480)
        // Fixed alignment established by the independent impulse above.
        // Decoded count deficit is recorded separately, not used as a delay.
        func error(lag: Int, start: Int, count: Int) -> Double {
            var residual = 0.0, energy = 0.0
            for frame in start..<(start + count) {
                for channel in 0..<2 {
                    let expected = Double(source[frame * 2 + channel])
                    let actual = Double(decoded[(frame + lag) * 2 + channel])
                    residual += pow(actual - expected, 2); energy += expected * expected
                }
            }
            return sqrt(residual / energy)
        }
        let unreturnedFrames = packets.count * 480 - decoded.count / 2
        print("oracle codedMinusDecodedFrames=\(unreturnedFrames), not classified as leading trim or tail")
        let lag = 192
        var worst = 0.0
        for start in stride(from: 1000, to: min(frames, decoded.count / 2 - lag) - 4800, by: 4800) {
            worst = max(worst, error(lag: lag, start: start, count: 4800))
        }
        let bitrate = Double(encoder.encodedBytes) * 8 / (Double(packets.count) * 0.01)
        print("production fragmentedPCM frames=\(frames) supplied=\(encoder.suppliedFrames) fragments=\(turn) packets=\(packets.count) preSkip=\(encoder.preSkip) measuredLag=\(lag) worstWindowNRMSE=\(worst) targetBitrate=128000 measuredCodedBitrate=\(bitrate)")
        precondition(worst < 0.15, "decoded content must remain continuous across fragmented input")
        print("OpusContinuityTests passed")
    }
}
