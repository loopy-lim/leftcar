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
private var peerCongestionCeilings: [UInt32: (ceiling: Int, savedAtNs: UInt64)] = [:]

/// Address is IPv4 host byte order, matching udpMediaPlaintextLimit callers.
/// A ceiling of zero means the session never confirmed congestion; nothing is
/// remembered then so healthy links keep the full ladder start.
func rememberPeerCongestionCeiling(addressHostOrder: UInt32, ceiling: Int, nowNs: UInt64) {
    guard ceiling > 0 else { return }
    peerCongestionLock.lock()
    peerCongestionCeilings[addressHostOrder] = (ceiling, nowNs)
    peerCongestionLock.unlock()
}

/// Nil once the memory is older than the TTL: a device returning after a
/// break, or moved to a better network, deserves the full ladder again.
func peerCongestionStartCeiling(
    addressHostOrder: UInt32,
    nowNs: UInt64,
    ttlNs: UInt64 = 10 * 60 * 1_000_000_000
) -> Int? {
    peerCongestionLock.lock()
    defer { peerCongestionLock.unlock() }
    guard let entry = peerCongestionCeilings[addressHostOrder],
          nowNs &- entry.savedAtNs < ttlNs else { return nil }
    return entry.ceiling
}
