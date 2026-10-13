import Foundation
import Darwin

/// Adaptive bitrate state lives inside a CaptureSession and dies with it, so
/// every viewer reconnect restarted the wire at the full w·h·fps·0.13 ideal
/// (~64Mbps at 4K60). A link that just congestion-collapsed re-collapses
/// before the 1Hz ABR can cut, and jittery-Wi-Fi viewers then cycle
/// stop/start through that collapse. Remember the congestion-cut ceiling per
/// media peer so the next session for the same device starts at the last
/// link-verified ceiling instead; the in-session raise ladder can still climb
/// past it once the link proves clean.
private let peerCongestionLock = NSLock()
private var peerCongestionCeilings = PeerCongestionMemory()

/// A long-running host can meet arbitrarily many peer addresses. TTL expiry
/// must release the entries, and a burst within the TTL must also stay bounded.
/// The outer lock protects the process-wide instance; local instances make
/// expiry and eviction deterministic in tests.
struct PeerCongestionMemory {
    static let defaultTtlNs: UInt64 = 10 * 60 * 1_000_000_000
    private let capacity: Int
    private var entries: [UInt32: (ceiling: Int, savedAtNs: UInt64)] = [:]

    init(capacity: Int = 128) {
        self.capacity = max(1, capacity)
    }

    var count: Int { entries.count }

    private mutating func expire(nowNs: UInt64, ttlNs: UInt64) {
        entries = entries.filter { _, entry in
            nowNs >= entry.savedAtNs && nowNs - entry.savedAtNs < ttlNs
        }
    }

    mutating func remember(address: UInt32, ceiling: Int, nowNs: UInt64) {
        guard ceiling > 0 else { return }
        expire(nowNs: nowNs, ttlNs: Self.defaultTtlNs)
        if entries[address] == nil, entries.count >= capacity,
           let oldest = entries.min(by: { left, right in
               left.value.savedAtNs == right.value.savedAtNs
                   ? left.key < right.key
                   : left.value.savedAtNs < right.value.savedAtNs
           })?.key {
            entries.removeValue(forKey: oldest)
        }
        entries[address] = (ceiling, nowNs)
    }

    mutating func startCeiling(
        address: UInt32, nowNs: UInt64, ttlNs: UInt64 = defaultTtlNs
    ) -> Int? {
        expire(nowNs: nowNs, ttlNs: ttlNs)
        return entries[address]?.ceiling
    }
}

/// Address is IPv4 host byte order, matching udpMediaPlaintextLimit callers.
/// A ceiling of zero means the session never confirmed congestion; nothing is
/// remembered then so healthy links keep the full ladder start.
func rememberPeerCongestionCeiling(addressHostOrder: UInt32, ceiling: Int, nowNs: UInt64) {
    guard ceiling > 0 else { return }
    peerCongestionLock.lock()
    peerCongestionCeilings.remember(address: addressHostOrder, ceiling: ceiling, nowNs: nowNs)
    peerCongestionLock.unlock()
}

/// Nil once the memory is older than the TTL: a device returning after a
/// break, or moved to a better network, deserves the full ladder again.
func peerCongestionStartCeiling(
    addressHostOrder: UInt32,
    nowNs: UInt64,
    ttlNs: UInt64 = PeerCongestionMemory.defaultTtlNs
) -> Int? {
    peerCongestionLock.lock()
    defer { peerCongestionLock.unlock() }
    return peerCongestionCeilings.startCeiling(address: addressHostOrder, nowNs: nowNs, ttlNs: ttlNs)
}
