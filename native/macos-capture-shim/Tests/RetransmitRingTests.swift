import Foundation

@main
struct RetransmitRingTests {
    static func main() {
        // Wire envelope: `G` | fragment_index u16 BE | fragment_count u16 BE
        // | au_id u16 LE | payload…
        func envelope(
            auID: UInt16,
            fragmentIndex: UInt16,
            fragmentCount: UInt16 = 1,
            marker: UInt8 = 0x47
        ) -> Data {
            var data = Data([marker])
            data.append(UInt8(fragmentIndex >> 8))
            data.append(UInt8(fragmentIndex & 0xFF))
            data.append(UInt8(fragmentCount >> 8))
            data.append(UInt8(fragmentCount & 0xFF))
            data.append(UInt8(auID & 0xFF))
            data.append(UInt8(auID >> 8))
            data.append(UInt8(fragmentIndex & 0xFF))
            return data
        }

        // au id is parsed little-endian from bytes 5-6: the byte-swapped
        // reading must stay empty while the id itself is retrievable.
        do {
            let ring = MediaRetransmitRing()
            let stored = envelope(auID: 0x0102, fragmentIndex: 3, fragmentCount: 9)
            ring.store(stored, side: nil)
            precondition(
                ring.lookup(auID: 0x0102, fragmentIndex: 3, side: nil) == stored,
                "au id must parse little-endian from bytes 5-6"
            )
            precondition(ring.lookup(auID: 0x0201, fragmentIndex: 3, side: nil) == nil)
            // Fragment keying: a missing fragment index within a stored AU.
            precondition(ring.lookup(auID: 0x0102, fragmentIndex: 4, side: nil) == nil)
        }

        // Sides are isolated: the same (au id, fragment) on .left and .right
        // each return their own envelope.
        do {
            let ring = MediaRetransmitRing()
            let left = envelope(auID: 5, fragmentIndex: 0)
            let right = envelope(auID: 5, fragmentIndex: 1)
            ring.store(left, side: .left)
            ring.store(right, side: .right)
            precondition(ring.lookup(auID: 5, fragmentIndex: 0, side: .left) == left)
            precondition(ring.lookup(auID: 5, fragmentIndex: 1, side: .left) == nil)
            precondition(ring.lookup(auID: 5, fragmentIndex: 1, side: .right) == right)
            precondition(ring.lookup(auID: 5, fragmentIndex: 0, side: .right) == nil)
        }

        // Non-DATA datagrams never enter the ring: parity marker, config
        // marker, and a `G` datagram shorter than the 8-byte header.
        do {
            let ring = MediaRetransmitRing()
            ring.store(envelope(auID: 7, fragmentIndex: 0, marker: 0x50), side: nil)
            ring.store(envelope(auID: 7, fragmentIndex: 1, marker: 0x43), side: nil)
            ring.store(Data([0x47, 0, 0, 0, 1, 7, 0]), side: nil)
            precondition(ring.lookup(auID: 7, fragmentIndex: 0, side: nil) == nil)
            precondition(ring.lookup(auID: 7, fragmentIndex: 1, side: nil) == nil)
        }

        // Oldest-AU FIFO eviction keeps the most recent 8 au ids per side.
        do {
            let ring = MediaRetransmitRing()
            for au in UInt16(0)...UInt16(9) {
                ring.store(envelope(auID: au, fragmentIndex: 0), side: nil)
            }
            precondition(ring.lookup(auID: 0, fragmentIndex: 0, side: nil) == nil)
            precondition(ring.lookup(auID: 1, fragmentIndex: 0, side: nil) == nil)
            precondition(ring.lookup(auID: 2, fragmentIndex: 0, side: nil) != nil)
            precondition(ring.lookup(auID: 9, fragmentIndex: 0, side: nil) != nil)
        }

        // Current-AU exception: an AU larger than the 512-entry cap is
        // retained whole while it is being stored.
        do {
            let ring = MediaRetransmitRing()
            for fragment in UInt16(0)..<UInt16(600) {
                ring.store(
                    envelope(auID: 100, fragmentIndex: fragment, fragmentCount: 600),
                    side: nil
                )
            }
            precondition(ring.lookup(auID: 100, fragmentIndex: 0, side: nil) != nil)
            precondition(ring.lookup(auID: 100, fragmentIndex: 599, side: nil) != nil)
        }

        // Entry-cap eviction: once a new AU arrives past the cap, the
        // oldest AUs are evicted whole until the count fits again.
        do {
            let ring = MediaRetransmitRing()
            for au in UInt16(200)...UInt16(205) {
                for fragment in UInt16(0)..<UInt16(100) {
                    ring.store(
                        envelope(auID: au, fragmentIndex: fragment, fragmentCount: 100),
                        side: nil
                    )
                }
            }
            // 600 entries over six AUs: the oldest AU (200) is evicted
            // whole, which brings the count back under the cap.
            precondition(ring.lookup(auID: 200, fragmentIndex: 0, side: nil) == nil)
            precondition(ring.lookup(auID: 201, fragmentIndex: 99, side: nil) != nil)
            precondition(ring.lookup(auID: 202, fragmentIndex: 0, side: nil) != nil)
            precondition(ring.lookup(auID: 205, fragmentIndex: 99, side: nil) != nil)
        }

        // An oversized AU is evicted whole by the next new AU: the
        // current-AU exception only protects the AU being stored.
        do {
            let ring = MediaRetransmitRing()
            for fragment in UInt16(0)..<UInt16(600) {
                ring.store(
                    envelope(auID: 100, fragmentIndex: fragment, fragmentCount: 600),
                    side: nil
                )
            }
            ring.store(envelope(auID: 101, fragmentIndex: 0), side: nil)
            precondition(ring.lookup(auID: 100, fragmentIndex: 0, side: nil) == nil)
            precondition(ring.lookup(auID: 100, fragmentIndex: 599, side: nil) == nil)
            precondition(ring.lookup(auID: 101, fragmentIndex: 0, side: nil) != nil)
        }

        // Sealed send contract: sealing adds exactly 24 wire bytes (8B
        // counter + 16B tag), a full sealed send is reported as the
        // plaintext length, and any other result passes through unchanged.
        do {
            guard let crypto = MediaSessionCrypto(
                mediaKey: Data((0..<32).map { UInt8($0) })
            ) else {
                preconditionFailure("media session crypto")
            }
            let plaintext = Data(repeating: 0x47, count: 37)
            guard let sealed = crypto.seal(plaintext) else {
                preconditionFailure("media seal")
            }
            precondition(sealed.count == plaintext.count + 24)
            precondition(
                normalizedSendResult(
                    sealed.count,
                    sealedCount: sealed.count,
                    plaintextCount: plaintext.count
                ) == plaintext.count
            )
            precondition(
                normalizedSendResult(
                    -1,
                    sealedCount: sealed.count,
                    plaintextCount: plaintext.count
                ) == -1
            )
            precondition(
                normalizedSendResult(
                    5,
                    sealedCount: sealed.count,
                    plaintextCount: plaintext.count
                ) == 5
            )
        }

        print("retransmit-ring tests passed")
    }
}
