import Foundation

struct EncodedTile<Value> {
    let side: TileSide
    let sequence: UInt64
    let value: Value
    let valid: Bool
}

enum EncodedPairAssemblyDecision<Value> {
    case wait
    case emit(left: Value, right: Value)
    case drop(requestPairedKeyframe: Bool)
}

extension EncodedPairAssemblyDecision: Equatable where Value: Equatable {}

private struct PendingEncodedPair<Value> {
    var left: Value?
    var right: Value?
    let firstReadyNs: UInt64

    var valueCount: Int {
        (left == nil ? 0 : 1) + (right == nil ? 0 : 1)
    }
}

struct EncodedPairAssembler<Value> {
    private let frameBudgetNs: UInt64
    private let maximumRetainedSequences: Int
    private var pending: [UInt64: PendingEncodedPair<Value>] = [:]

    init(frameBudgetNs: UInt64, maximumRetainedSequences: Int = 2) {
        precondition(frameBudgetNs > 0)
        precondition(maximumRetainedSequences > 0)
        self.frameBudgetNs = frameBudgetNs
        self.maximumRetainedSequences = maximumRetainedSequences
    }

    var retainedSequenceCount: Int { pending.count }

    var retainedValueCount: Int {
        pending.values.reduce(into: 0) { count, pair in
            count += pair.valueCount
        }
    }

    mutating func insert(
        _ tile: EncodedTile<Value>,
        nowNs: UInt64
    ) -> EncodedPairAssemblyDecision<Value> {
        guard tile.valid else {
            pending.removeAll(keepingCapacity: true)
            return .drop(requestPairedKeyframe: true)
        }

        var pair = pending[tile.sequence] ?? PendingEncodedPair(firstReadyNs: nowNs)
        switch tile.side {
        case .left:
            guard pair.left == nil else {
                pending.removeAll(keepingCapacity: true)
                return .drop(requestPairedKeyframe: true)
            }
            pair.left = tile.value
        case .right:
            guard pair.right == nil else {
                pending.removeAll(keepingCapacity: true)
                return .drop(requestPairedKeyframe: true)
            }
            pair.right = tile.value
        }
        pending[tile.sequence] = pair

        if let left = pair.left, let right = pair.right {
            pending.removeValue(forKey: tile.sequence)
            return .emit(left: left, right: right)
        }

        guard pending.count <= maximumRetainedSequences else {
            pending.removeAll(keepingCapacity: true)
            return .drop(requestPairedKeyframe: true)
        }
        return .wait
    }

    mutating func expire(nowNs: UInt64) -> EncodedPairAssemblyDecision<Value> {
        let expired = pending.values.contains { pair in
            nowNs >= pair.firstReadyNs && nowNs - pair.firstReadyNs >= frameBudgetNs
        }
        guard expired else { return .wait }
        pending.removeAll(keepingCapacity: true)
        return .drop(requestPairedKeyframe: true)
    }

    mutating func reset() {
        pending.removeAll(keepingCapacity: true)
    }
}
