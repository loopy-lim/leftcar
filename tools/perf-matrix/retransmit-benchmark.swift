import Foundation
@main struct RetransmitBenchmark {
    static func main() {
        for count in [50, 100, 317, 634, 1268, 1600] {
            let packets: [Data] = (0..<count).map { index in
                var bytes = [UInt8](repeating: 7, count: 1400)
                bytes[0] = 0x47
                bytes[1] = UInt8(index >> 8); bytes[2] = UInt8(index & 255)
                bytes[3] = UInt8(count >> 8); bytes[4] = UInt8(count & 255)
                bytes[5] = 1; bytes[6] = 0
                return Data(bytes)
            }
            let start = DispatchTime.now().uptimeNanoseconds
            var retained = 0
            var storeNs: UInt64 = 0
            var lookupNs: UInt64 = 0
            for _ in 0..<50 {
                let ring = MediaRetransmitRing()
                let storeStart = DispatchTime.now().uptimeNanoseconds
                for packet in packets { ring.store(packet, side: .left) }
                let lookupStart = DispatchTime.now().uptimeNanoseconds
                storeNs += lookupStart - storeStart
                retained = (0..<count).filter { ring.lookup(auID: 1, fragmentIndex: UInt16($0), side: .left) != nil }.count
                lookupNs += DispatchTime.now().uptimeNanoseconds - lookupStart
            }
            let ns = (DispatchTime.now().uptimeNanoseconds - start) / 50
            print("{\"fragments\":\(count),\"retained\":\(retained),\"retainedBytes\":\(retained * 1400),\"nsPerStoreLookupRound\":\(ns),\"nsStore\":\(storeNs / 50),\"nsLookup\":\(lookupNs / 50)}")
            if CommandLine.arguments.contains("--assert-bounded") && count == 1600 {
                precondition(retained == 0, "oversized current AU must be rejected whole")
            }
        }
    }
}
