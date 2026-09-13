import Foundation
import AudioToolbox

@main struct OpusInputContractTests {
    static func main() throws {
        let pump = OpusPacketPump(channels: 1, maximumPacket: 16)
        var loan: UnsafeMutableRawPointer?
        var phase = 0
        let first = try pump.appendPCM([1,2,3,4,5,6,7,8]) { input, produced, output, description in
            defer { phase += 1 }
            if phase == 0 {
                var requested: UInt32 = 2
                var data = AudioBufferList()
                precondition(input.provide(&requested, &data) == noErr && requested == 2)
                loan = data.mBuffers.mData
                precondition(input.suppliedFrames == 2 && input.pendingBytes == 4)
            } else if phase == 1 {
                // Buffered packet: no callback and no consumption of the rest.
                precondition(input.suppliedFrames == 2 && input.pendingBytes == 4)
            } else { produced = 0; return noErr }
            output.mBuffers.mData!.storeBytes(of: UInt8(phase + 1), as: UInt8.self)
            description.mDataByteSize = 1
            return noErr
        }
        precondition(first.map(Array.init) == [[1], [2]])
        // Appending fragmented input must not alter or free the converter loan.
        pump.input.append([9,10,11])
        precondition(Array(UnsafeRawBufferPointer(start: loan!, count: 4)) == [1,2,3,4])
        precondition(pump.input.suppliedFrames == 2 && pump.input.pendingBytes == 7)
        var supplied: [UInt8] = []
        let second = try pump.appendPCM([12]) { input, produced, output, description in
            for amount: UInt32 in [1, 2, 4] {
                var requested = amount
                var data = AudioBufferList()
                precondition(input.provide(&requested, &data) == noErr)
                supplied += Array(UnsafeRawBufferPointer(start: data.mBuffers.mData, count: Int(data.mBuffers.mDataByteSize)))
            }
            var requested: UInt32 = 1
            var data = AudioBufferList()
            let status = input.provide(&requested, &data)
            precondition(status == opusNeedsInput && requested == 0 && data.mBuffers.mData == nil)
            // Valid buffered output may accompany temporary shortage.
            output.mBuffers.mData!.storeBytes(of: UInt8(3), as: UInt8.self)
            description.mDataByteSize = 1
            return status
        }
        precondition(supplied == [5,6,7,8,9,10,11,12])
        precondition(second.map(Array.init) == [[3]])
        precondition(pump.input.suppliedFrames == 6 && pump.input.pendingBytes == 0)
        print("OpusInputContractTests passed: retained loan, output/input independence, multiple requests, fragments, temporary shortage")
    }
}
