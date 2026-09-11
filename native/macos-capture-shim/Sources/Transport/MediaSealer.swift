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
/// them. Each direction gets its own key derived from the viewer-generated
/// session key (`MediaKeyDerivation.directionalKeys`, mirroring
/// `secure_channel::media_keys`): sharing the raw key would collide
/// viewer→host frame #N with host→viewer frame #N under the same
/// (key, nonce) on every session. The send counter starts at a random point
/// per instance so a reconfigure that rebuilds the sealers under the reused
/// session key still cannot repeat a nonce.
struct MediaSealer {
    private let key: SymmetricKey
    private var sendCounter: UInt64
    private var highestSeen: UInt64 = 0
    private var window = [UInt64](repeating: 0, count: Int(MediaWireFormat.replayWindow / 64))

    init?(mediaKey: Data) {
        guard mediaKey.count == 32 else { return nil }
        self.key = SymmetricKey(data: mediaKey)
        // 62비트 난수 시작(1 이상) — 인스턴스 쌍당 2^-62로 시작점이 겹친다.
        self.sendCounter = UInt64.random(in: 1..<(1 << 62))
    }

    /// 12-byte ChaChaPoly nonce: 4 zero bytes then the big-endian counter.
    private static func nonce(counter: UInt64) -> [UInt8] {
        var nonce = [UInt8](repeating: 0, count: 12)
        let counterBE = counter.bigEndian
        withUnsafeBytes(of: counterBE) { raw in
            for index in 0..<8 {
                nonce[4 + index] = raw[index]
            }
        }
        return nonce
    }

    /// `counter ‖ tag ‖ ct` or nil when the plaintext exceeds the datagram cap.
    mutating func seal(_ plaintext: Data) -> Data? {
        guard plaintext.count <= MediaWireFormat.maxDatagram else { return nil }
        sendCounter &+= 1
        let counter = sendCounter
        let nonce = Self.nonce(counter: counter)
        guard let sealedBox = try? ChaChaPoly.seal(
            plaintext,
            using: key,
            nonce: ChaChaPoly.Nonce(data: nonce)
        ) else { return nil }
        let counterBE = counter.bigEndian
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
        let nonce = Self.nonce(counter: counter)
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

/// HKDF-SHA256 방향별 키 도출 — `crates/secure-channel`의
/// `media_keys`와 바이트 단위로 같아야 한다(ikm = 세션 키, salt 없음 =
/// RFC 5869 0바이트 32개, info = "leftcar/media/v1", okm 64B = c2s ‖ s2c).
/// 고정 벡터: 세션 키 00..1f →
/// c2s 5608c4ec91f01a93afdd876da3419cafd5fc6862faaa6c15a008b16dab6ac72f
/// s2c 2c100b32a507ab3af07ec3d39a61df7072d1d97e22d0e43da19d44fabedaf182
enum MediaKeyDerivation {
    static let info = Data("leftcar/media/v1".utf8)

    static func directionalKeys(mediaKey: Data) -> (c2s: Data, s2c: Data)? {
        guard mediaKey.count == 32 else { return nil }
        // RFC 5869 extract: salt가 없으면 해시 길이의 0바이트가 키다.
        let salt = Data(repeating: 0, count: 32)
        let prk = Data(HMAC<SHA256>.authenticationCode(
            for: mediaKey, using: SymmetricKey(data: salt)
        ))
        var okm = Data()
        var block = Data()
        var counter: UInt8 = 1
        while okm.count < 64 {
            block = Data(HMAC<SHA256>.authenticationCode(
                for: block + info + [counter], using: SymmetricKey(data: prk)
            ))
            okm += block
            counter += 1
        }
        return (Data(okm.prefix(32)), Data(okm.suffix(32)))
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
        guard let keys = MediaKeyDerivation.directionalKeys(mediaKey: mediaKey),
              let toViewer = MediaSealer(mediaKey: keys.s2c),
              let fromViewer = MediaSealer(mediaKey: keys.c2s) else {
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
