import Foundation
import CoreGraphics

/// Polling rate for the LCD1 cursor stream — the same 2x-stream-FPS policy
/// the viewer's LCI1 pointer plane uses (`InputScheduler::polling_rate_hz`),
/// clamped to a bounded datagram budget. A zero or tiny FPS falls back to the
/// 30Hz floor instead of dividing by zero.
func cursorPollingHz(fps: UInt32) -> UInt32 {
    guard fps < 120 else { return 240 }
    return max(30, min(240, fps &* 2))
}

/// LCD1 cursor wire format:
/// `LCD1 | sequence u32 BE | x u16 BE | y u16 BE | visibility u8 | shape u8 |
/// token`. Coordinates arrive normalized to the capture content rect as
/// 0...65535. Byte layout mirrors the viewer's `cursor_protocol.rs` parser;
/// the two must stay in lockstep.
func encodeCursorPacket(
    sequence: UInt32,
    x: UInt16,
    y: UInt16,
    visible: Bool,
    token: Data
) -> Data {
    var bytes = Data(capacity: 14 + token.count)
    bytes.append(Data("LCD1".utf8))
    bytes.append(UInt8(truncatingIfNeeded: sequence >> 24))
    bytes.append(UInt8(truncatingIfNeeded: sequence >> 16))
    bytes.append(UInt8(truncatingIfNeeded: sequence >> 8))
    bytes.append(UInt8(truncatingIfNeeded: sequence))
    bytes.append(UInt8(truncatingIfNeeded: x >> 8))
    bytes.append(UInt8(truncatingIfNeeded: x))
    bytes.append(UInt8(truncatingIfNeeded: y >> 8))
    bytes.append(UInt8(truncatingIfNeeded: y))
    bytes.append(visible ? 1 : 0)
    // Shape is reserved; the MVP streams position only.
    bytes.append(0)
    bytes.append(token)
    return bytes
}

/// Newest-wins cursor state with change detection. `packetDue` coalesces
/// CGEvent-tap observations into at most one LCD1 datagram per polling tick,
/// mirroring the viewer's `InputScheduler` pointer path in reverse: no
/// reliability layer, the newest sample is simply the truth. A packet is
/// produced only when the cursor state changed since the last send, and only
/// once an authenticated session token is known — samples are never emitted
/// unauthenticated.
///
/// Single-owner contract: this is a value type whose mutable state must not
/// fork. Exactly one owner (in the wiring task, the capture session's cursor
/// lock domain) holds and mutates the instance; copying it would split the
/// sequence space, and the stale copy's samples would then be dropped as
/// regressions by the viewer's newest-wins ordering.
struct CursorStreamCoordinator {
    private let pollingIntervalUs: UInt64
    private let bounds: CGRect
    private var token: Data?
    private var enabled = false
    private var sequence: UInt32 = 0
    private var lastSentUs: UInt64 = 0
    private var dirty = false
    private var hasSample = false
    private var position = CGPoint.zero
    private var visible = false

    init(fps: UInt32, bounds: CGRect) {
        pollingIntervalUs = UInt64(1_000_000 / Int64(cursorPollingHz(fps: fps)))
        self.bounds = bounds
    }

    /// The session token travels inside every LCD1 packet so the viewer can
    /// bind samples to the authenticated session. Until it arrives, `packetDue`
    /// stays silent. Installing a different token means a freshly
    /// authenticated viewer that never saw the last sample, so the current
    /// state is flushed instead of waiting for the next cursor change.
    mutating func setToken(_ newToken: Data) {
        let sanitized = newToken.isEmpty ? nil : newToken
        if sanitized != token {
            dirty = dirty || hasSample
        }
        token = sanitized
    }

    mutating func setEnabled(_ newValue: Bool) {
        guard enabled != newValue else { return }
        enabled = newValue
        if newValue {
            // Flush an immediate sample so the viewer paints the cursor as
            // soon as it opts in — but only when a real observation exists.
            // Enabling a stream that never saw the cursor stays silent rather
            // than inventing a (0, 0, hidden) sample.
            dirty = hasSample
            lastSentUs = 0
        }
    }

    /// Records a CGEvent-tap observation in global screen coordinates. The
    /// observation is recorded whether or not the stream is enabled — that
    /// keeps the latest truth current, so a later enable flushes the real
    /// position instead of a stale pre-disable one. Normalization against the
    /// capture content rect happens once, at packet time.
    mutating func note(position newPosition: CGPoint, visible newVisible: Bool) {
        if position != newPosition || visible != newVisible {
            dirty = true
        }
        hasSample = true
        position = newPosition
        visible = newVisible
    }

    /// Returns the encoded LCD1 datagram when fresh state is due, else nil.
    /// `nowUs` is a monotonic uptime clock in microseconds (for example
    /// `DispatchTime.now().uptimeNanoseconds / 1_000`); `lastSentUs == 0` is
    /// the never-sent sentinel, which relies on an uptime clock never
    /// reporting exactly zero at a live call site.
    mutating func packetDue(nowUs: UInt64) -> Data? {
        guard enabled, dirty, let token,
              lastSentUs == 0 || nowUs >= lastSentUs + pollingIntervalUs
        else { return nil }
        dirty = false
        lastSentUs = nowUs
        // Sequence wrap back to 0 poisons the stream until the session
        // rebinds — the accepted LCI1 tradeoff shared with the viewer's
        // pointer plane.
        sequence = sequence &+ 1
        return encodeCursorPacket(
            sequence: sequence,
            x: normalized(position.x, origin: bounds.origin.x, extent: bounds.width),
            y: normalized(position.y, origin: bounds.origin.y, extent: bounds.height),
            visible: visible,
            token: token
        )
    }

    /// Map one axis of a global screen coordinate onto the captured content
    /// rect as 0...65535, clamping anything outside the rect. Non-finite
    /// coordinates collapse to 0 — mirroring the viewer's `normalized_axis`
    /// guard — because a NaN would otherwise trap the `UInt16` conversion and
    /// kill the host capture process.
    private func normalized(_ value: CGFloat, origin: CGFloat, extent: CGFloat) -> UInt16 {
        guard value.isFinite, extent > 0 else { return 0 }
        let fraction = (value - origin) / extent
        return UInt16((min(max(fraction, 0), 1) * 65535).rounded())
    }
}
