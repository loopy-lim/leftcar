import Foundation
import CryptoKit

/// Wire format shared with `crates/secure-channel` (Rust) and the Android
/// viewer: `counter u64 BE ‖ poly1305 tag 16B ‖ ciphertext`. The nonce is
/// `00 00 00 00 ‖ counter u64 BE` (12B), so ChaCha20-Poly1305 (CryptoKit
/// `ChaChaPoly`) interoperates byte-for-byte with the Rust and TS sealers.
enum MediaWireFormat {
    static let counterLen = 8
    static let tagLen = 16
    /// UDP-sealed plaintext cap — one fragmented media datagram.
    static let maxDatagram = 64 * 1024
    /// Allowed UDP reorder width, mirroring `secure_channel::REPLAY_WINDOW`.
    static let replayWindow: UInt64 = 8192
}

/// One directional UDP sealer. The host holds an s2c instance for everything
/// it sends and a c2s instance for everything it receives; the viewer mirrors
/// them. Both directions deliberately derive nothing and use the very same
/// viewer-generated 32-byte key: the send counters of the two directions are
/// independent sequences and the Poly1305 tag binds the ciphertext content,
/// so an identical (counter, key) pair across directions cannot collide into
/// a reused nonce on a meaningful message.
struct MediaSealer {
    private let key: SymmetricKey
    private var sendCounter: UInt64 = 0
    private var highestSeen: UInt64 = 0
    private var window = [UInt64](repeating: 0, count: Int(MediaWireFormat.replayWindow / 64))

    init?(mediaKey: Data) {
        guard mediaKey.count == 32 else { return nil }
        self.key = SymmetricKey(data: mediaKey)
    }

    /// `counter ‖ tag ‖ ct` or nil when the plaintext exceeds the datagram cap.
    mutating func seal(_ plaintext: Data) -> Data? {
        guard plaintext.count <= MediaWireFormat.maxDatagram else { return nil }
        sendCounter &+= 1
        let counter = sendCounter
        var nonce = [UInt8](repeating: 0, count: 12)
        let counterBE = counter.bigEndian
        withUnsafeBytes(of: counterBE) { raw in
            for index in 0..<8 {
                nonce[4 + index] = raw[index]
            }
        }
        guard let sealedBox = try? ChaChaPoly.seal(
            plaintext,
            using: key,
            nonce: ChaChaPoly.Nonce(data: nonce)
        ) else { return nil }
        var frame = Data(capacity: MediaWireFormat.counterLen + MediaWireFormat.tagLen + plaintext.count)
        withUnsafeBytes(of: counterBE) { frame.append(contentsOf: $0) }
        frame.append(sealedBox.tag)
        frame.append(sealedBox.ciphertext)
        return frame
    }

    /// Open a sealed frame under the RFC 6479 sliding replay window. Returns
    /// the plaintext, or nil for forged/replayed/truncated frames — callers
    /// drop those without further parsing.
    mutating func open(_ frame: Data) -> Data? {
        guard frame.count >= MediaWireFormat.counterLen + MediaWireFormat.tagLen else { return nil }
        let bytes = Array(frame)
        let counter: UInt64 = (UInt64(bytes[0]) << 56) | (UInt64(bytes[1]) << 48)
            | (UInt64(bytes[2]) << 40) | (UInt64(bytes[3]) << 32)
            | (UInt64(bytes[4]) << 24) | (UInt64(bytes[5]) << 16)
            | (UInt64(bytes[6]) << 8) | UInt64(bytes[7])
        guard counter != 0 else { return nil }
        if !check(counter: counter) { return nil }
        // Fresh Data copies: Data slices retain a non-zero startIndex, which
        // CryptoKit's SealedBox does not accept and later Collection code
        // must not assume away. Layout is counter ‖ tag ‖ ct, so the
        // ciphertext is what follows the counter+tag prefix.
        let ciphertext = Data(frame.dropFirst(
            MediaWireFormat.counterLen + MediaWireFormat.tagLen
        ))
        let tag = Data(frame.prefix(MediaWireFormat.counterLen + MediaWireFormat.tagLen)
            .suffix(MediaWireFormat.tagLen))
        var nonce = [UInt8](repeating: 0, count: 12)
        let counterBE = counter.bigEndian
        withUnsafeBytes(of: counterBE) { raw in
            for index in 0..<8 {
                nonce[4 + index] = raw[index]
            }
        }
        let sealedBox: ChaChaPoly.SealedBox
        do {
            sealedBox = try ChaChaPoly.SealedBox(
                nonce: ChaChaPoly.Nonce(data: nonce),
                ciphertext: ciphertext,
                tag: tag
            )
            let plaintext = try ChaChaPoly.open(sealedBox, using: key)
            accept(counter: counter)
            return plaintext
        } catch {
            return nil
        }
    }

    /// RFC 6479 bitmap over the last `replayWindow` counters below the high
    /// watermark. Age 0 (the watermark itself) and beyond-window counters are
    /// always replays.
    private mutating func check(counter: UInt64) -> Bool {
        if counter > highestSeen { return true }
        let age = highestSeen - counter
        if age == 0 || age >= MediaWireFormat.replayWindow { return false }
        let word = Int(age / 64)
        let bit = UInt64(age % 64)
        return window[word] & (1 << bit) == 0
    }

    private mutating func accept(counter: UInt64) {
        guard counter > highestSeen else {
            let age = highestSeen - counter
            if age >= 1 && age < MediaWireFormat.replayWindow {
                let word = Int(age / 64)
                let bit = UInt64(age % 64)
                window[word] |= 1 << bit
            }
            return
        }
        let delta = counter - highestSeen
        if delta >= MediaWireFormat.replayWindow {
            for index in window.indices { window[index] = 0 }
        } else {
            shiftBits(Int(delta))
            let word = Int(delta / 64)
            let bit = UInt64(delta % 64)
            window[word] |= 1 << bit
        }
        highestSeen = counter
    }

    private mutating func shiftBits(_ shift: Int) {
        let wordShift = shift / 64
        let bitShift = UInt64(shift % 64)
        if wordShift >= window.count {
            for index in window.indices { window[index] = 0 }
            return
        }
        var next = [UInt64](repeating: 0, count: window.count)
        for index in wordShift..<window.count {
            let source = index - wordShift
            next[index] = window[source] << bitShift
            if bitShift > 0 && source >= 1 {
                next[index] |= window[source - 1] >> (64 - bitShift)
            }
        }
        window = next
    }
}

/// Thread-safe pair of directional sealers owned by one capture session.
/// `sealFromViewer` opens viewer → host frames; `sealToViewer` seals every
/// host → viewer frame. Both live for the whole session so the counters
/// never reset across transport switches within the session.
final class MediaSessionCrypto {
    private let lock = NSLock()
    private var toViewer: MediaSealer
    private var fromViewer: MediaSealer

    init?(mediaKey: Data) {
        guard let toViewer = MediaSealer(mediaKey: mediaKey),
              let fromViewer = MediaSealer(mediaKey: mediaKey) else {
            return nil
        }
        self.toViewer = toViewer
        self.fromViewer = fromViewer
    }

    func seal(_ plaintext: Data) -> Data? {
        lock.lock()
        defer { lock.unlock() }
        return toViewer.seal(plaintext)
    }

    func open(_ frame: Data) -> Data? {
        lock.lock()
        defer { lock.unlock() }
        return fromViewer.open(frame)
    }
}
