import Foundation

enum TileSide: Int, CaseIterable, Hashable {
    case left
    case right
}

struct TilePlaneRegion: Equatable {
    let lumaX: Int
    let chromaX: Int
    let width: Int
    let height: Int

    var chromaWidth: Int { width / 2 }
    var chromaHeight: Int { height / 2 }
}

struct SplitGeometry: Equatable {
    let fullWidth: Int
    let fullHeight: Int
    let left: TilePlaneRegion
    let right: TilePlaneRegion

    static let vertical4K = SplitGeometry(
        fullWidth: 3_840,
        fullHeight: 2_160,
        left: TilePlaneRegion(
            lumaX: 0,
            chromaX: 0,
            width: 1_920,
            height: 2_160
        ),
        right: TilePlaneRegion(
            lumaX: 1_920,
            chromaX: 960,
            width: 1_920,
            height: 2_160
        )
    )
}

enum PairAdmissionDecision: Equatable {
    case admitPair
    case dropPair
}

struct PairAdmissionState: Equatable {
    let leftAvailable: Bool
    let rightAvailable: Bool

    var decision: PairAdmissionDecision {
        leftAvailable && rightAvailable ? .admitPair : .dropPair
    }
}

struct SplitFrameIdentity: Equatable, Hashable {
    let sequence: UInt64
    let ptsUs: Int64
}
