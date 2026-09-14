import Foundation
import CryptoKit

/// MediaSealer 봉인 레이아웃을 crates/secure-channel의 고정 벡터와 바이트
/// 단위로 잠근다. 태그가 앞(옛 잘못된 배치)에 오면 이 벡터가 실패한다 —
/// 그 배치에서는 호스트↔뷰어 미디어·입력 인증이 전부 죽는다.
@main
struct MediaSealerTests {
    static func main() {
        let key = Data((0..<32).map { $0 })
        let counter: UInt64 = 0x0102_0304_0506_0708
        let plaintext = Data("leftcar media vector v1".utf8)

        // cargo test -p secure-channel -- print_media_vector --ignored --nocapture
        guard var sealer = MediaSealer(mediaKey: key, firstCounter: counter) else {
            fatalError("sealer rejected a 32-byte key")
        }
        guard let frame = sealer.seal(plaintext) else {
            fatalError("seal failed")
        }
        let expected = Data(
            [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08] +
                [
                    0x83, 0x83, 0xc3, 0x8c, 0xc6, 0xed, 0xda, 0xbc,
                    0x7d, 0xda, 0x8c, 0xbf, 0xeb, 0xcc, 0x51, 0x3b,
                    0xf6, 0xf8, 0x0b, 0x23, 0x3c, 0x3b, 0x19, 0xda,
                ] +
                [
                    0x38, 0x07, 0x97, 0x49, 0xaa, 0xf6, 0xba, 0xa6,
                    0x52, 0xf6, 0xb5, 0xc2, 0x87, 0x74, 0x68,
                ]
        )
        guard frame == expected else {
            fatalError("cross-language vector mismatch — layout drifted from Rust counter|ct|tag")
        }

        // 자기 봉인 프레임은 역방향 인스턴스에서 열려야 한다(카운터 접두 8B,
        // 뒤 16B 태그, 중간 암호문).
        guard var opener = MediaSealer(mediaKey: key, firstCounter: 1) else {
            fatalError("opener rejected a 32-byte key")
        }
        guard let opened = opener.open(frame), opened == plaintext else {
            fatalError("open failed on a validly sealed frame")
        }
        // 재생된 카운터는 거부된다.
        if opener.open(frame) != nil {
            fatalError("replayed frame must be rejected")
        }

        // 낡은 배치(counter ‖ tag ‖ ct)로 재배열한 프레임은 인증에 실패해야
        // 한다 — 태그 위치가 바뀌면 open이 우연히 성공해선 안 된다. 위의
        // opener는 이 카운터를 이미 소비했으니 새 인스턴스로 재생 아닌
        // 인증 실패만을 검사한다.
        let counterBytes = Array(frame.prefix(8))
        let tag = Array(frame.suffix(16))
        let ciphertext = Array(frame.dropFirst(8).dropLast(16))
        let legacyLayout = Data(counterBytes + tag + ciphertext)
        guard var second = MediaSealer(mediaKey: key, firstCounter: 1) else {
            fatalError("second opener rejected a 32-byte key")
        }
        if second.open(legacyLayout) != nil {
            fatalError("misplaced-tag frame must fail authentication")
        }

        print("MediaSealerTests: cross-language vector + replay + tag-order OK")
    }
}
