import CoreGraphics
import Foundation

enum AdaptiveMotionMode: String, Equatable {
    case interactive
    case video
}

struct DirtyRegionMotionSample: Equatable {
    let rectCount: Int
    let changedPixelRatio: Double
}

struct AdaptiveMotionObservation: Equatable {
    let mode: AdaptiveMotionMode
    let enteredVideo: Bool
    let enteredInteractive: Bool
}

struct AdaptiveMotionState {
    private static let broadChangeRatio = 0.20
    private static let dirtyRegionHoldNs: UInt64 = 800_000_000
    private static let accessUnitHoldNs: UInt64 = 2_000_000_000

    private(set) var highMotionUntilNs: UInt64 = 0
    private var lastObservedMode = AdaptiveMotionMode.interactive

    func mode(at nowNs: UInt64) -> AdaptiveMotionMode {
        highMotionUntilNs > nowNs ? .video : .interactive
    }

    mutating func observe(
        nowNs: UInt64,
        changedPixelRatio: Double?,
        accessUnitPressure: Bool
    ) -> AdaptiveMotionObservation {
        let modeBeforeEvidence = mode(at: nowNs)
        let expiredSinceLastObservation = lastObservedMode == .video
            && modeBeforeEvidence == .interactive
        var holdNs: UInt64 = 0
        if let changedPixelRatio,
           changedPixelRatio.isFinite,
           changedPixelRatio >= Self.broadChangeRatio {
            holdNs = Self.dirtyRegionHoldNs
        }
        if accessUnitPressure {
            holdNs = max(holdNs, Self.accessUnitHoldNs)
        }
        if holdNs > 0 {
            let deadline = nowNs.addingReportingOverflow(holdNs)
            highMotionUntilNs = max(
                highMotionUntilNs,
                deadline.overflow ? .max : deadline.partialValue
            )
        }
        let nextMode = mode(at: nowNs)
        lastObservedMode = nextMode
        return AdaptiveMotionObservation(
            mode: nextMode,
            enteredVideo: modeBeforeEvidence != .video && nextMode == .video,
            enteredInteractive: expiredSinceLastObservation && nextMode == .interactive
        )
    }
}

func dirtyRegionMotionSample(
    rects: [CGRect],
    frameWidth: Int,
    frameHeight: Int
) -> DirtyRegionMotionSample {
    guard frameWidth > 0, frameHeight > 0 else {
        return DirtyRegionMotionSample(rectCount: 0, changedPixelRatio: 0)
    }
    let bounds = CGRect(x: 0, y: 0, width: frameWidth, height: frameHeight)
    let clipped = rects.compactMap { rect -> CGRect? in
        guard rect.origin.x.isFinite,
              rect.origin.y.isFinite,
              rect.size.width.isFinite,
              rect.size.height.isFinite else {
            return nil
        }
        let intersection = rect.standardized.intersection(bounds)
        guard !intersection.isNull, !intersection.isEmpty else { return nil }
        return intersection
    }
    guard !clipped.isEmpty else {
        return DirtyRegionMotionSample(rectCount: 0, changedPixelRatio: 0)
    }

    let xEdges = Array(Set(clipped.flatMap { [$0.minX, $0.maxX] })).sorted()
    var unionArea = 0.0
    for index in 0..<(xEdges.count - 1) {
        let left = xEdges[index]
        let right = xEdges[index + 1]
        guard right > left else { continue }
        var intervals = [(CGFloat, CGFloat)]()
        for rect in clipped where rect.minX < right && rect.maxX > left {
            intervals.append((rect.minY, rect.maxY))
        }
        intervals.sort { lhs, rhs in
            lhs.0 == rhs.0 ? lhs.1 < rhs.1 : lhs.0 < rhs.0
        }
        guard var current = intervals.first else { continue }
        var coveredHeight = 0.0
        for interval in intervals.dropFirst() {
            if interval.0 <= current.1 {
                current.1 = max(current.1, interval.1)
            } else {
                coveredHeight += current.1 - current.0
                current = interval
            }
        }
        coveredHeight += current.1 - current.0
        unionArea += (right - left) * coveredHeight
    }

    let frameArea = Double(frameWidth) * Double(frameHeight)
    let ratio = min(1, max(0, unionArea / frameArea))
    return DirtyRegionMotionSample(
        rectCount: clipped.count,
        changedPixelRatio: ratio
    )
}

func motionAdjustedUdpBurstDatagrams(
    base: Int,
    mode: AdaptiveMotionMode
) -> Int {
    mode == .video ? max(8, base) : max(1, base)
}

func highMotionBitrateTarget(
    current: Int,
    floor: Int,
    ceiling: Int,
    motionFloor: Int
) -> Int {
    min(ceiling, max(current, max(floor, motionFloor)))
}

extension CaptureSession {
    func observeCaptureMotion(
        _ sample: DirtyRegionMotionSample,
        nowNs: UInt64
    ) {
        stateLock.lock()
        lastDirtyRectCount = UInt32(min(sample.rectCount, Int(UInt32.max)))
        lastDirtyChangedPixelRatio = sample.changedPixelRatio
        let observation = adaptiveMotionState.observe(
            nowNs: nowNs,
            changedPixelRatio: sample.changedPixelRatio,
            accessUnitPressure: false
        )
        if observation.enteredVideo || observation.enteredInteractive {
            adaptiveMotionTransitions &+= 1
        }
        if observation.enteredVideo {
            adaptiveMotionEvidence = "dirty_regions"
        } else if observation.enteredInteractive {
            adaptiveMotionEvidence = "localized"
        }
        stateLock.unlock()
        scheduleMotionBitrateAdaptationIfNeeded(observation)
    }

    func observeAccessUnitMotion(
        nowNs: UInt64,
        highMotion: Bool
    ) -> AdaptiveMotionObservation {
        stateLock.lock()
        let observation = adaptiveMotionState.observe(
            nowNs: nowNs,
            changedPixelRatio: nil,
            accessUnitPressure: highMotion
        )
        if observation.enteredVideo || observation.enteredInteractive {
            adaptiveMotionTransitions &+= 1
        }
        if observation.enteredVideo {
            adaptiveMotionEvidence = "access_unit"
        } else if observation.enteredInteractive {
            adaptiveMotionEvidence = "localized"
        }
        stateLock.unlock()
        scheduleMotionBitrateAdaptationIfNeeded(observation)
        return observation
    }

    private func scheduleMotionBitrateAdaptationIfNeeded(
        _ observation: AdaptiveMotionObservation
    ) {
        guard observation.enteredVideo else { return }
        encodeQueue.async { [weak self] in
            self?.adaptBitrateIfNeeded()
        }
    }
}
