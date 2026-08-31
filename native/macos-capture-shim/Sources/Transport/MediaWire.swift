import Foundation

struct PendingEncodedFrame {
    let data: Data
    let isKeyframe: Bool
    let isRecoveryKeyframe: Bool
    let tileSide: TileSide?
    let queuedNs: UInt64

    init(
        data: Data,
        isKeyframe: Bool,
        isRecoveryKeyframe: Bool,
        tileSide: TileSide? = nil,
        queuedNs: UInt64 = DispatchTime.now().uptimeNanoseconds
    ) {
        self.data = data
        self.isKeyframe = isKeyframe
        self.isRecoveryKeyframe = isRecoveryKeyframe
        self.tileSide = tileSide
        self.queuedNs = queuedNs
    }
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
