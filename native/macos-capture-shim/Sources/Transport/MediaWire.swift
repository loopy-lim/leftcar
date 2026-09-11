import Foundation

struct PendingEncodedFrame {
    let data: Data
    let isKeyframe: Bool
    let isRecoveryKeyframe: Bool
    // var 옵셔널은 멤버와이즈 이니셜라이저에서 nil 기본 인자를 받는다.
    var tileSide: TileSide?
    var queuedNs: UInt64 = DispatchTime.now().uptimeNanoseconds
}

struct NetworkQueueSnapshot {
    let count: Int
    let bytes: Int
    let oldestAgeUs: UInt64
}

func networkQueueSnapshot(
    frames: [PendingEncodedFrame],
    splitAccessUnits: [PendingSplitAccessUnit] = [],
    nowNs: UInt64
) -> NetworkQueueSnapshot {
    let frameBytes = frames.reduce(into: 0) { total, frame in
        total += frame.data.count
    }
    let splitBytes = splitAccessUnits.reduce(into: 0) { total, accessUnit in
        total += accessUnit.bytes
    }
    let frameOldestNs = frames.map(\.queuedNs).min()
    let splitOldestNs = splitAccessUnits.map(\.queuedNs).min()
    let oldestNs = [frameOldestNs, splitOldestNs].compactMap { $0 }.min()
    let oldestAgeUs = oldestNs
        .map { frame in
            nowNs >= frame ? (nowNs - frame) / 1_000 : 0
        }
        ?? 0
    return NetworkQueueSnapshot(
        count: frames.count + splitAccessUnits.count * 2,
        bytes: frameBytes + splitBytes,
        oldestAgeUs: oldestAgeUs
    )
}
