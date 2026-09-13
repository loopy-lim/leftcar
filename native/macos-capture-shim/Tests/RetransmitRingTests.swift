import Foundation

private func require(_ condition: @autoclosure () -> Bool, _ message: String = "", line: UInt = #line) {
    if !condition() {
        FileHandle.standardError.write(Data("RTX assertion failed at line \(line): \(message)\n".utf8))
        exit(1)
    }
}

@main
struct RetransmitRingTests {
    static func main() {
        // Wire envelope: `G` | fragment_index u16 BE | fragment_count u16 BE
        // | au_id u16 LE | payload…
        nonisolated func envelope(
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
            require(
                ring.lookup(auID: 0x0102, fragmentIndex: 3, side: nil) == stored,
                "au id must parse little-endian from bytes 5-6"
            )
            require(ring.lookup(auID: 0x0201, fragmentIndex: 3, side: nil) == nil)
            // Fragment keying: a missing fragment index within a stored AU.
            require(ring.lookup(auID: 0x0102, fragmentIndex: 4, side: nil) == nil)
        }

        // Sides are isolated: the same (au id, fragment) on .left and .right
        // each return their own envelope.
        do {
            let ring = MediaRetransmitRing()
            let left = envelope(auID: 5, fragmentIndex: 0)
            let right = envelope(auID: 5, fragmentIndex: 1, fragmentCount: 2)
            ring.store(left, side: .left)
            ring.store(right, side: .right)
            require(ring.lookup(auID: 5, fragmentIndex: 0, side: .left) == left)
            require(ring.lookup(auID: 5, fragmentIndex: 1, side: .left) == nil)
            require(ring.lookup(auID: 5, fragmentIndex: 1, side: .right) == right)
            require(ring.lookup(auID: 5, fragmentIndex: 0, side: .right) == nil)
        }

        // Non-DATA datagrams never enter the ring: parity marker, config
        // marker, and a `G` datagram shorter than the 8-byte header.
        do {
            let ring = MediaRetransmitRing()
            ring.store(envelope(auID: 7, fragmentIndex: 0, marker: 0x50), side: nil)
            ring.store(envelope(auID: 7, fragmentIndex: 1, marker: 0x43), side: nil)
            ring.store(Data([0x47, 0, 0, 0, 1, 7, 0]), side: nil)
            require(ring.lookup(auID: 7, fragmentIndex: 0, side: nil) == nil)
            require(ring.lookup(auID: 7, fragmentIndex: 1, side: nil) == nil)
        }

        // Oldest-AU FIFO eviction keeps the most recent 8 au ids per side.
        do {
            let ring = MediaRetransmitRing()
            for au in UInt16(0)...UInt16(9) {
                ring.store(envelope(auID: au, fragmentIndex: 0), side: nil)
            }
            require(ring.lookup(auID: 0, fragmentIndex: 0, side: nil) == nil)
            require(ring.lookup(auID: 1, fragmentIndex: 0, side: nil) == nil)
            require(ring.lookup(auID: 2, fragmentIndex: 0, side: nil) != nil)
            require(ring.lookup(auID: 9, fragmentIndex: 0, side: nil) != nil)
        }

        // Byte budgets reject the entire oversized generation, including
        // fragments arriving after eviction, after other IDs, and after age.
        do {
            var now: UInt64 = 0
            let budget = MediaRetransmitBudget(limits: .init(totalEnvelopeBytes: 40, accessUnitEnvelopeBytes: 24, maxAgeNanoseconds: 100, accessUnits: 256), clock: { now })
            let ring = MediaRetransmitRing(budget: budget)
            for index in UInt16(0)..<4 { ring.store(envelope(auID: 100, fragmentIndex: index, fragmentCount: 4), side: nil) }
            require(budget.statistics().retainedEnvelopeBytes == 0)
            require(budget.statistics().evictedEnvelopeBytes == 24)
            require(budget.statistics().rejectedEnvelopeBytes == 8)
            ring.store(envelope(auID: 100, fragmentIndex: 0, fragmentCount: 4), side: nil)
            ring.store(envelope(auID: 101, fragmentIndex: 0), side: nil)
            now = 100
            ring.store(envelope(auID: 100, fragmentIndex: 3, fragmentCount: 4), side: nil)
            require(ring.lookup(auID: 100, fragmentIndex: 0, side: nil) == nil)
            require(ring.lookup(auID: 100, fragmentIndex: 3, side: nil) == nil)
            require(budget.statistics().retainedEnvelopeBytes == 0)
        }

        // Sides and separate session owners compete for one budget. Evicting
        // an older current AU must not make its next fragment a new AU.
        do {
            let budget = MediaRetransmitBudget(limits: .init(totalEnvelopeBytes: 24, accessUnitEnvelopeBytes: 24, maxAgeNanoseconds: 100, accessUnits: 256), clock: { 0 })
            let a = MediaRetransmitRing(budget: budget)
            var b: MediaRetransmitRing? = MediaRetransmitRing(budget: budget)
            a.store(envelope(auID: 1, fragmentIndex: 0, fragmentCount: 2), side: .left)
            a.store(envelope(auID: 1, fragmentIndex: 1, fragmentCount: 2), side: .left)
            a.store(envelope(auID: 1, fragmentIndex: 0), side: .right)
            b!.store(envelope(auID: 1, fragmentIndex: 0), side: .left)
            require(a.lookup(auID: 1, fragmentIndex: 0, side: .left) == nil)
            require(a.lookup(auID: 1, fragmentIndex: 1, side: .left) == nil)
            a.store(envelope(auID: 1, fragmentIndex: 1, fragmentCount: 2), side: .left)
            require(a.lookup(auID: 1, fragmentIndex: 1, side: .left) == nil)
            require(b!.lookup(auID: 1, fragmentIndex: 0, side: .left) != nil)
            require(a.lookup(auID: 1, fragmentIndex: 0, side: .right) != nil)
            let stats = budget.statistics()
            require(stats.retainedEnvelopeBytes == 16 && stats.retainedAccessUnits == 2)
            require(stats.evictedEnvelopeBytes == 16 && stats.evictedAccessUnits == 1)
            require(stats.servedEnvelopeBytes == 16 && stats.missedFragments == 3)
            require(stats.missedRequestedBytesUpperBound == 4200)
            b = nil
            require(budget.statistics().retainedEnvelopeBytes == 8)
        }

        // Expiry is from the first fragment, not refreshed by duplicates or
        // reads; operations purge idle entries before they can be served.
        do {
            var now: UInt64 = 0
            let budget = MediaRetransmitBudget(limits: .init(totalEnvelopeBytes: 64, accessUnitEnvelopeBytes: 64, maxAgeNanoseconds: 100, accessUnits: 256), clock: { now })
            let ring = MediaRetransmitRing(budget: budget)
            let data = envelope(auID: 1, fragmentIndex: 0, fragmentCount: 2)
            ring.store(data, side: nil)
            now = 99
            ring.store(data, side: nil)
            require(ring.lookup(auID: 1, fragmentIndex: 0, side: nil) == data)
            require(budget.statistics().retainedEnvelopeBytes == 8)
            now = 100
            require(ring.lookup(auID: 1, fragmentIndex: 0, side: nil) == nil)
            ring.store(envelope(auID: 1, fragmentIndex: 1, fragmentCount: 2), side: nil)
            require(ring.lookup(auID: 1, fragmentIndex: 1, side: nil) == nil)
            require(budget.statistics().evictedEnvelopeBytes == 8)
        }

        // Activity in another session expires old global entries but keeps
        // newer entries alive; identical IDs belong to distinct session owners.
        do {
            var now: UInt64 = 0
            let budget = MediaRetransmitBudget(limits: .init(totalEnvelopeBytes: 64, accessUnitEnvelopeBytes: 64, maxAgeNanoseconds: 100, accessUnits: 256), clock: { now })
            let older = MediaRetransmitRing(budget: budget)
            let newer = MediaRetransmitRing(budget: budget)
            older.store(envelope(auID: 1, fragmentIndex: 0), side: nil)
            now = 50
            newer.store(envelope(auID: 1, fragmentIndex: 0), side: nil)
            now = 100
            require(newer.lookup(auID: 1, fragmentIndex: 0, side: nil) != nil)
            require(older.lookup(auID: 1, fragmentIndex: 0, side: nil) == nil)
            withExtendedLifetime((older, newer)) {
                require(budget.statistics().retainedEnvelopeBytes == 8 && budget.statistics().evictedEnvelopeBytes == 8)
            }
        }

        // Reject malformed header counts, inconsistent counts and envelopes
        // beyond the plaintext cap without leaving a partial recoverable AU.
        do {
            let ring = MediaRetransmitRing()
            ring.store(envelope(auID: 1, fragmentIndex: 1), side: nil)
            ring.store(envelope(auID: 1, fragmentIndex: 0, fragmentCount: 0), side: nil)
            require(ring.lookup(auID: 1, fragmentIndex: 1, side: nil) == nil)
            ring.store(envelope(auID: 2, fragmentIndex: 0, fragmentCount: 2), side: nil)
            ring.store(envelope(auID: 2, fragmentIndex: 1, fragmentCount: 3), side: nil)
            require(ring.lookup(auID: 2, fragmentIndex: 0, side: nil) == nil)
            var oversized = envelope(auID: 3, fragmentIndex: 0)
            oversized.append(Data(repeating: 0, count: 1393))
            ring.store(oversized, side: nil)
            ring.store(envelope(auID: 3, fragmentIndex: 0), side: nil)
            require(ring.lookup(auID: 3, fragmentIndex: 0, side: nil) == nil)
            // Non-zero Data startIndex must not affect endian parsing.
            let sliced = (Data([9]) + envelope(auID: 4, fragmentIndex: 0)).dropFirst()
            ring.store(sliced, side: nil)
            require(ring.lookup(auID: 4, fragmentIndex: 0, side: nil) == sliced)
        }

        // Complete serial wrap permits legitimate same-ID reuse. An old ID
        // outside the retained window cannot resurrect after FIFO eviction.
        do {
            let budget = MediaRetransmitBudget(clock: { 0 })
            let ring = MediaRetransmitRing(budget: budget)
            for id in UInt32(0)...UInt32(65536) {
                ring.store(envelope(auID: UInt16(truncatingIfNeeded: id), fragmentIndex: 0), side: nil)
            }
            require(ring.lookup(auID: 0, fragmentIndex: 0, side: nil) != nil)
            require(ring.lookup(auID: 65535, fragmentIndex: 0, side: nil) != nil)
            ring.store(envelope(auID: 65500, fragmentIndex: 0), side: nil)
            require(ring.lookup(auID: 65500, fragmentIndex: 0, side: nil) == nil)
            require(budget.statistics().retainedAccessUnits == 8)
        }

        // Existing session owners concurrently store fragments and inspect
        // shared accounting under pressure. Cleanup is sequential afterward.
        do {
            let budget = MediaRetransmitBudget(limits: .init(totalEnvelopeBytes: 800, accessUnitEnvelopeBytes: 32, maxAgeNanoseconds: 100, accessUnits: 32), clock: { 0 })
            var rings: [MediaRetransmitRing] = (0..<200).map { _ in MediaRetransmitRing(budget: budget) }
            DispatchQueue.concurrentPerform(iterations: rings.count) { i in
                for fragment in UInt16(0)..<4 {
                    rings[i].store(envelope(auID: 1, fragmentIndex: fragment, fragmentCount: 4), side: i % 2 == 0 ? .left : .right)
                    let stats = budget.statistics()
                    require(stats.retainedEnvelopeBytes <= 800 && stats.retainedAccessUnits <= 32)
                }
            }
            require(budget.statistics().retainedEnvelopeBytes <= 800)
            rings.removeAll()
            require(budget.statistics().retainedEnvelopeBytes == 0 && budget.statistics().retainedAccessUnits == 0)
        }

        // Global metadata cap applies even when tiny envelopes fit the bytes.
        do {
            let budget = MediaRetransmitBudget(limits: .init(totalEnvelopeBytes: 8000, accessUnitEnvelopeBytes: 80, maxAgeNanoseconds: 100, accessUnits: 2), clock: { 0 })
            let rings = (0..<3).map { _ in MediaRetransmitRing(budget: budget) }
            for ring in rings { ring.store(envelope(auID: 1, fragmentIndex: 0), side: nil) }
            require(rings[0].lookup(auID: 1, fragmentIndex: 0, side: nil) == nil)
            withExtendedLifetime(rings) {
                require(budget.statistics().retainedAccessUnits == 2 && budget.statistics().retainedEnvelopeBytes == 16)
            }
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
            require(sealed.count == plaintext.count + 24)
            require(
                normalizedSendResult(
                    sealed.count,
                    sealedCount: sealed.count,
                    plaintextCount: plaintext.count
                ) == plaintext.count
            )
            require(
                normalizedSendResult(
                    -1,
                    sealedCount: sealed.count,
                    plaintextCount: plaintext.count
                ) == -1
            )
            require(
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
