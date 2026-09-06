import Foundation

struct AdaptiveResolutionTarget: Equatable {
    let width: Int
    let height: Int
    let fps: Int
}

struct AdaptiveResolutionObservation {
    let receiverLossDelta: Int
    let encodedFps: Double
    let transmittedFps: Double
    let requestedFps: Double
    let queueAgeUs: UInt64
    let latencyBudgetUs: UInt64
    let recoveryActive: Bool
    let rebindInFlight: Bool
    var floorCollapseDelta: Int = 0
}

enum AdaptiveResolutionDecision: Equatable {
    case keep
    case downshift(AdaptiveResolutionTarget)
    case upshift(AdaptiveResolutionTarget)
}

/// Why the encoded stream's shape changed. Host status records this so an
/// operator can tell a resolution fallback from an ordinary bitrate move.
enum AdaptiveResolutionTransitionKind: Equatable {
    case bitrate_changed
    case resolution_changed

    init(previousActive: AdaptiveResolutionTarget, nextActive: AdaptiveResolutionTarget) {
        self = previousActive == nextActive
            ? .bitrate_changed
            : .resolution_changed
    }
}

/// ABR interaction gate for a 4K stream. Once the congestion floor has
/// consumed the whole bitrate budget, the rate controller cannot restore the
/// frame rate on its own; the resolution fallback must take over instead of
/// letting transmitted FPS sink toward zero. A sub-4K stream always keeps its
/// floor decision because it has no resolution fallback.
enum AdaptiveBitrateFloorDecision: Equatable {
    case keepFloor
    case downshiftTo1440p
}

func adaptiveFallbackTarget(for source: AdaptiveResolutionTarget) -> AdaptiveResolutionTarget? {
    let portrait = source.height > source.width
    let orientedMaxWidth = portrait ? 1_440.0 : 2_560.0
    let orientedMaxHeight = portrait ? 2_560.0 : 1_440.0
    let scaleToFit = min(
        min(orientedMaxWidth / Double(source.width), orientedMaxHeight / Double(source.height)),
        1.0
    )
    guard scaleToFit < 1 else { return nil }
    let shortSide = Double(min(source.width, source.height))
    let scale = min(1.0, max(scaleToFit, 720.0 / shortSide))
    let width = max(2, Int(Double(source.width) * scale) / 2 * 2)
    let height = max(2, Int(Double(source.height) * scale) / 2 * 2)
    guard Double(width) <= orientedMaxWidth, Double(height) <= orientedMaxHeight else { return nil }
    guard width < source.width || height < source.height else { return nil }
    return AdaptiveResolutionTarget(width: width, height: height, fps: source.fps)
}

func adaptiveBitrateFloorDecision(
    activeWidth: Int,
    activeHeight: Int,
    floorBitrate: Int,
    currentBitrate: Int
) -> AdaptiveBitrateFloorDecision {
    let hasResolutionFallback = adaptiveFallbackTarget(
        for: AdaptiveResolutionTarget(width: activeWidth, height: activeHeight, fps: 60)
    ) != nil
    // Within 10% of the congestion floor the rate controller has no more
    // budget to trade; waiting longer only sinks transmitted FPS.
    let floorReached = Double(currentBitrate)
        <= Double(floorBitrate) * 1.10
    guard hasResolutionFallback, floorReached else {
        return .keepFloor
    }
    return .downshiftTo1440p
}

struct AdaptiveResolutionPolicy {
    static let downshiftWindows = 2
    static let upshiftWindows = 4
    static let rebindCooldownMs: UInt64 = 5_000

    let sourceTarget: AdaptiveResolutionTarget
    let fallbackTarget: AdaptiveResolutionTarget?
    private(set) var activeTarget: AdaptiveResolutionTarget
    private(set) var congestionWindows = 0
    private(set) var stableWindows = 0
    private(set) var cooldownUntilMs: UInt64 = 0

    init(sourceTarget: AdaptiveResolutionTarget) {
        self.sourceTarget = sourceTarget
        self.activeTarget = sourceTarget
        self.fallbackTarget = adaptiveFallbackTarget(for: sourceTarget)
    }

    mutating func observe(
        nowMs: UInt64,
        observation: AdaptiveResolutionObservation
    ) -> AdaptiveResolutionDecision {
        if observation.recoveryActive || observation.rebindInFlight {
            congestionWindows = 0
            stableWindows = 0
            return .keep
        }

        if activeTarget == sourceTarget {
            if nowMs < cooldownUntilMs {
                congestionWindows = 0
                stableWindows = 0
                return .keep
            }
            let fpsCollapsed = observation.encodedFps < observation.requestedFps * 0.9
                || observation.transmittedFps < observation.requestedFps * 0.9
            let congested = observation.floorCollapseDelta > 0
                || observation.queueAgeUs > observation.latencyBudgetUs
                || (observation.receiverLossDelta > 0 && fpsCollapsed)
            congestionWindows = congested ? congestionWindows + 1 : 0
            stableWindows = 0
            if congestionWindows >= Self.downshiftWindows, let fallbackTarget {
                return .downshift(fallbackTarget)
            }
            return .keep
        }

        let healthy = observation.receiverLossDelta == 0
            && observation.encodedFps >= observation.requestedFps * 0.95
            && observation.queueAgeUs <= observation.latencyBudgetUs
            && !observation.recoveryActive
        stableWindows = healthy ? stableWindows + 1 : 0
        congestionWindows = 0
        if stableWindows >= Self.upshiftWindows, nowMs >= cooldownUntilMs {
            return .upshift(sourceTarget)
        }
        return .keep
    }

    mutating func record(
        decision: AdaptiveResolutionDecision,
        success: Bool,
        nowMs: UInt64,
        acceptedTarget: AdaptiveResolutionTarget? = nil
    ) {
        let target: AdaptiveResolutionTarget
        switch decision {
        case .downshift(let next), .upshift(let next):
            target = next
        case .keep:
            return
        }
        if success {
            activeTarget = acceptedTarget ?? target
        }
        congestionWindows = 0
        stableWindows = 0
        cooldownUntilMs = nowMs + Self.rebindCooldownMs
    }
}
