import Foundation
import AudioToolbox

/// A temporary lack of input is not EOF. Fill may still return valid output
/// together with this callback status; the next append resumes the converter.
let opusNeedsInput: OSStatus = 0x746D7069 // 'tmpi', private callback status

/// Serial converter-owner storage. Only a subsequent callback may overwrite
/// the last loan, never append/accumulator compaction or a Fill return.
final class OpusPCMInput {
    let channels: Int
    private var pending: [UInt8] = []
    private var loan: UnsafeMutableRawPointer?
    private var loanCapacity = 0
    private(set) var suppliedFrames: UInt64 = 0
    var pendingBytes: Int { pending.count }
    init(channels: Int) { self.channels = channels }
    deinit { loan?.deallocate() }
    func append(_ bytes: [UInt8]) { pending.append(contentsOf: bytes) }
    func provide(_ packets: inout UInt32, _ data: inout AudioBufferList) -> OSStatus {
        let frameBytes = channels * 2
        let count = min(Int(packets), pending.count / frameBytes)
        packets = UInt32(count)
        data.mNumberBuffers = 1
        guard count > 0 else {
            data.mBuffers = AudioBuffer(mNumberChannels: UInt32(channels), mDataByteSize: 0, mData: nil)
            return opusNeedsInput
        }
        let bytes = count * frameBytes
        if bytes > loanCapacity {
            loan?.deallocate()
            loan = .allocate(byteCount: bytes, alignment: MemoryLayout<Int16>.alignment)
            loanCapacity = bytes
        }
        pending.withUnsafeBytes { source in loan!.copyMemory(from: source.baseAddress!, byteCount: bytes) }
        pending.removeFirst(bytes)
        suppliedFrames += UInt64(count)
        data.mBuffers = AudioBuffer(mNumberChannels: UInt32(channels), mDataByteSize: UInt32(bytes), mData: loan)
        return noErr
    }
}

let opusPCMInput: AudioConverterComplexInputDataProc = { _, packets, data, _, context in
    guard let context else { return -50 }
    let input = Unmanaged<OpusPCMInput>.fromOpaque(context).takeUnretainedValue()
    return input.provide(&packets.pointee, &data.pointee)
}

/// Production-consumed pump: input advances in the callback only, independently
/// of output packet count. The adapter may request input repeatedly or return
/// buffered output without a new input callback.
final class OpusPacketPump {
    typealias Fill = (OpusPCMInput, inout UInt32, inout AudioBufferList, inout AudioStreamPacketDescription) -> OSStatus
    let input: OpusPCMInput
    private var output: [UInt8]
    init(channels: Int, maximumPacket: Int) {
        input = OpusPCMInput(channels: channels)
        output = [UInt8](repeating: 0, count: maximumPacket)
    }
    func appendPCM(_ pcm: [UInt8], fill: Fill) throws -> [Data] {
        input.append(pcm)
        var packets: [Data] = []
        while true {
            var produced: UInt32 = 1
            var description = AudioStreamPacketDescription()
            let status = output.withUnsafeMutableBytes { destination in
                var list = AudioBufferList(mNumberBuffers: 1,
                    mBuffers: AudioBuffer(mNumberChannels: UInt32(input.channels), mDataByteSize: UInt32(destination.count), mData: destination.baseAddress))
                return fill(input, &produced, &list, &description)
            }
            guard status == noErr || status == opusNeedsInput else { throw OpusAudioEncoder.CodecError.status(status) }
            guard produced <= 1 else { throw OpusAudioEncoder.CodecError.invalid }
            if produced == 1 {
                guard description.mStartOffset == 0, description.mDataByteSize > 0,
                    description.mDataByteSize <= output.count else { throw OpusAudioEncoder.CodecError.invalid }
                packets.append(Data(output.prefix(Int(description.mDataByteSize))))
            }
            if status == opusNeedsInput || produced == 0 { break }
        }
        return packets
    }
}
