import Foundation

/// One process-wide owner for plaintext RTX payloads across every capture
/// session and side. Defaults are initial safety budgets, not measured optima.
/// Byte limits count retained Data envelope lengths, not process RSS or
/// Data backing allocations. The packetizer currently creates independent
/// <=1400-byte envelopes. A future caller retaining slices of large buffers
/// must revisit that assumption; copying here would add a packet-copy cost.
/// Expiry is lazy on store/lookup/statistics: idle bytes remain capped and
/// expired data cannot be served. The injected clock must be monotonic and
/// must not reenter this budget. All state, including ring side maps, uses
/// this one lock; no ring-local lock or reverse lock order exists.
final class MediaRetransmitBudget {
    struct Limits {
        var totalEnvelopeBytes = 6 * 1024 * 1024
        var accessUnitEnvelopeBytes = 2 * 1024 * 1024
        var maxAgeNanoseconds: UInt64 = 250_000_000
        var accessUnits = 256
    }
    struct Statistics {
        var retainedEnvelopeBytes = 0
        var retainedAccessUnits = 0
        var servedEnvelopeBytes: UInt64 = 0
        var missedFragments: UInt64 = 0
        // NAK carries indexes, not lengths. This is a bound, never measured
        // lost payload bytes; each miss adds the accepted envelope cap.
        var missedRequestedBytesUpperBound: UInt64 = 0
        var evictedEnvelopeBytes: UInt64 = 0
        var evictedAccessUnits: UInt64 = 0
        var rejectedEnvelopeBytes: UInt64 = 0
    }
    static let shared = MediaRetransmitBudget()
    fileprivate let lock = NSLock()
    fileprivate let limits: Limits
    fileprivate let clock: () -> UInt64
    fileprivate var counters = Statistics()
    private var first: RetransmitAccessUnit?
    private var last: RetransmitAccessUnit?

    init(limits: Limits = Limits(), clock: @escaping () -> UInt64 = { DispatchTime.now().uptimeNanoseconds }) {
        precondition(limits.totalEnvelopeBytes > 0 && limits.accessUnitEnvelopeBytes > 0 && limits.accessUnits > 0 && limits.maxAgeNanoseconds > 0)
        self.limits = limits
        self.clock = clock
    }

    func statistics() -> Statistics {
        lock.lock()
        defer { lock.unlock() }
        expire(at: clock())
        return counters
    }

    fileprivate func append(_ au: RetransmitAccessUnit) {
        au.previous = last
        last?.next = au
        if first == nil { first = au }
        last = au
        counters.retainedAccessUnits += 1
    }

    fileprivate func remove(_ au: RetransmitAccessUnit, eviction: Bool = true) {
        guard !au.removed else { return }
        if let previous = au.previous { previous.next = au.next } else { first = au.next }
        if let next = au.next { next.previous = au.previous } else { last = au.previous }
        au.previous = nil
        au.next = nil
        counters.retainedAccessUnits -= 1
        counters.retainedEnvelopeBytes -= au.bytes
        if eviction {
            counters.evictedEnvelopeBytes &+= UInt64(au.bytes)
            counters.evictedAccessUnits &+= 1
        }
        au.fragments.removeAll(keepingCapacity: false)
        au.bytes = 0
        au.removed = true
        au.onRemoval?()
        au.onRemoval = nil
    }

    fileprivate func expire(at now: UInt64) {
        while let oldest = first,
              now >= oldest.createdAt,
              now - oldest.createdAt >= limits.maxAgeNanoseconds {
            remove(oldest)
        }
    }

    fileprivate func makeRoom(bytes: Int = 0, newUnit: Bool = false) {
        while counters.retainedEnvelopeBytes > limits.totalEnvelopeBytes - bytes ||
              (newUnit && counters.retainedAccessUnits >= limits.accessUnits) {
            guard let oldest = first else { break }
            remove(oldest)
        }
    }

}

/// Reference storage avoids per-insert dictionary copy-on-write; linked global
/// FIFO makes each append/removal O(1), including a partially built current AU.
fileprivate final class RetransmitAccessUnit {
    let id: UInt16
    let fragmentCount: UInt16
    let createdAt: UInt64
    var fragments: [UInt16: Data] = [:]
    var bytes = 0
    var removed = false
    var onRemoval: (() -> Void)?
    weak var previous: RetransmitAccessUnit?
    var next: RetransmitAccessUnit?
    init(id: UInt16, fragmentCount: UInt16, createdAt: UInt64) {
        self.id = id
        self.fragmentCount = fragmentCount
        self.createdAt = createdAt
    }
}

/// Session-owned cache; default construction joins the shared process budget.
/// Whole AU removal invalidates every cached fragment and all subsequent stores
/// of that generation. Each side retains one UInt16 high watermark until
/// session destruction. Removal releases the AU record from its side map;
/// there are <=256 active records globally and <=8 per side, with <=3 small
/// side maps per session. No tombstone set grows with evictions or idle time.
/// Missing older IDs are never readmitted. Forward serial deltas 1..<32768
/// admit new AUs, including 65535->0. Same-ID reuse after a full advancing wrap
/// is valid. A session reset must construct a new ring; idle time alone never
/// revives a rejected generation. As on the wire, delays spanning a full UInt16
/// cycle cannot be distinguished without a protocol generation field.
final class MediaRetransmitRing {
    static let maxAccessUnitsPerSide = 8
    // Packetizer's direct-LAN plaintext limit is udpMediaDatagramBytes=1400.
    // Enforce it here too, making missedRequestedBytesUpperBound truthful even
    // for callers outside that packetizer (oversized envelopes are rejected).
    static let maxEnvelopeBytes = 1_400
    private final class SideState {
        var latest: UInt16?
        var units: [UInt16: RetransmitAccessUnit] = [:]
        var order: [UInt16] = []
    }
    private let budget: MediaRetransmitBudget
    private var sides: [Int: SideState] = [:]
    init(budget: MediaRetransmitBudget = .shared) { self.budget = budget }
    deinit {
        budget.lock.lock()
        defer { budget.lock.unlock() }
        for side in sides.values {
            while let unit = side.units.values.first { budget.remove(unit, eviction: false) }
        }
    }
    private static func sideKey(_ side: TileSide?) -> Int {
        // Keep nil independent as well as left/right, even if a caller changes
        // modes during a session. Per-session ownership never aliases.
        side.map { $0 == .left ? 0 : 1 } ?? 2
    }

    func store(_ envelope: Data, side: TileSide?) {
        let start = envelope.startIndex
        guard envelope.count > 7, envelope[start] == 0x47 else { return }
        let index = (UInt16(envelope[start + 1]) << 8) | UInt16(envelope[start + 2])
        let count = (UInt16(envelope[start + 3]) << 8) | UInt16(envelope[start + 4])
        let id = UInt16(envelope[start + 5]) | (UInt16(envelope[start + 6]) << 8)
        guard count > 0, index < count else { return }
        budget.lock.lock()
        defer { budget.lock.unlock() }
        let now = budget.clock()
        budget.expire(at: now)
        let key = Self.sideKey(side)
        let state = sides[key] ?? SideState()
        sides[key] = state
        let unit: RetransmitAccessUnit
        if let existing = state.units[id] {
            unit = existing
        } else {
            if let latest = state.latest {
                let delta = id &- latest
                guard delta > 0 && delta < 32768 else {
                    budget.counters.rejectedEnvelopeBytes &+= UInt64(envelope.count)
                    return
                }
            }
            state.latest = id
            while state.order.count >= Self.maxAccessUnitsPerSide {
                let oldest = state.order[0]
                if let old = state.units[oldest] { budget.remove(old) }
            }
            unit = RetransmitAccessUnit(id: id, fragmentCount: count, createdAt: now)
            unit.onRemoval = { [weak state] in
                state?.units.removeValue(forKey: id)
                state?.order.removeAll { $0 == id }
            }
            budget.makeRoom(newUnit: true)
            state.units[id] = unit
            state.order.append(id)
            budget.append(unit)
        }
        guard !unit.removed else {
            budget.counters.rejectedEnvelopeBytes &+= UInt64(envelope.count)
            return
        }
        guard count == unit.fragmentCount, envelope.count <= Self.maxEnvelopeBytes else {
            budget.remove(unit)
            budget.counters.rejectedEnvelopeBytes &+= UInt64(envelope.count)
            return
        }
        // A repeated index does not refresh age, replace bytes, or increase
        // the reservation. First send owns the immutable retransmit envelope.
        guard unit.fragments[index] == nil else { return }
        guard envelope.count <= budget.limits.accessUnitEnvelopeBytes - unit.bytes,
              envelope.count <= budget.limits.totalEnvelopeBytes - unit.bytes else {
            budget.remove(unit)
            budget.counters.rejectedEnvelopeBytes &+= UInt64(envelope.count)
            return
        }
        budget.makeRoom(bytes: envelope.count)
        guard !unit.removed else {
            budget.counters.rejectedEnvelopeBytes &+= UInt64(envelope.count)
            return
        }
        unit.fragments[index] = envelope
        unit.bytes += envelope.count
        budget.counters.retainedEnvelopeBytes += envelope.count
    }

    func lookup(auID: UInt16, fragmentIndex: UInt16, side: TileSide?) -> Data? {
        budget.lock.lock()
        defer { budget.lock.unlock() }
        budget.expire(at: budget.clock())
        let data = sides[Self.sideKey(side)]?.units[auID]?.fragments[fragmentIndex]
        if let data {
            budget.counters.servedEnvelopeBytes &+= UInt64(data.count)
        } else {
            budget.counters.missedFragments &+= 1
            budget.counters.missedRequestedBytesUpperBound &+= UInt64(Self.maxEnvelopeBytes)
        }
        return data
    }
}

/// Full sealed send reports the original plaintext length; errors and short
/// writes preserve their existing result for both TCP and UDP callers.
func normalizedSendResult(_ result: Int, sealedCount: Int, plaintextCount: Int) -> Int {
    result == sealedCount ? plaintextCount : result
}
