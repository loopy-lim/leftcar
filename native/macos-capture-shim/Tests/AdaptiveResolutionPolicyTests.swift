import Foundation

@main
struct AdaptiveResolutionPolicyTests {
    static func main() {
        let source = AdaptiveResolutionTarget(width: 3_840, height: 2_160, fps: 60)
        let bad = AdaptiveResolutionObservation(
            receiverLossDelta: 1,
            encodedFps: 40,
            transmittedFps: 40,
            requestedFps: 60,
            queueAgeUs: 10_000,
            latencyBudgetUs: 100_000,
            recoveryActive: false,
            rebindInFlight: false
        )
        var policy = AdaptiveResolutionPolicy(sourceTarget: source)
        precondition(policy.observe(nowMs: 1_000, observation: bad) == .keep)
        let fallback = AdaptiveResolutionTarget(width: 2_560, height: 1_440, fps: 60)
        precondition(policy.observe(nowMs: 2_000, observation: bad) == .downshift(fallback))
        policy.record(decision: .downshift(fallback), success: true, nowMs: 2_000)

        let recoveryLoss = AdaptiveResolutionObservation(
            receiverLossDelta: 20,
            encodedFps: 1,
            transmittedFps: 1,
            requestedFps: 60,
            queueAgeUs: 1_000_000,
            latencyBudgetUs: 100_000,
            recoveryActive: true,
            rebindInFlight: false
        )
        precondition(policy.observe(nowMs: 3_000, observation: recoveryLoss) == .keep)
        policy.record(decision: .upshift(source), success: false, nowMs: 3_000)
        precondition(policy.activeTarget == fallback)

        let queuePressure = AdaptiveResolutionObservation(
            receiverLossDelta: 0, encodedFps: 60, transmittedFps: 60,
            requestedFps: 60, queueAgeUs: 120_000, latencyBudgetUs: 100_000,
            recoveryActive: false, rebindInFlight: false
        )
        var queuePolicy = AdaptiveResolutionPolicy(sourceTarget: source)
        precondition(queuePolicy.observe(nowMs: 1_000, observation: queuePressure) == .keep)
        precondition(queuePolicy.observe(nowMs: 2_000, observation: queuePressure) == .downshift(fallback))
        let idle = AdaptiveResolutionObservation(
            receiverLossDelta: 0, encodedFps: 1, transmittedFps: 1,
            requestedFps: 60, queueAgeUs: 1_000, latencyBudgetUs: 100_000,
            recoveryActive: false, rebindInFlight: false
        )
        var idlePolicy = AdaptiveResolutionPolicy(sourceTarget: source)
        precondition(idlePolicy.observe(nowMs: 1_000, observation: idle) == .keep)
        precondition(idlePolicy.observe(nowMs: 2_000, observation: idle) == .keep)

        let wide = AdaptiveResolutionPolicy(sourceTarget: .init(width: 3_200, height: 2_000, fps: 60))
        precondition(wide.fallbackTarget == .init(width: 2_304, height: 1_440, fps: 60))
        let portrait = AdaptiveResolutionPolicy(sourceTarget: .init(width: 2_000, height: 3_200, fps: 60))
        precondition(portrait.fallbackTarget == .init(width: 1_440, height: 2_304, fps: 60))

        policy.record(decision: .downshift(fallback), success: true, nowMs: 5_000)
        let healthy = AdaptiveResolutionObservation(
            receiverLossDelta: 0,
            encodedFps: 60,
            transmittedFps: 60,
            requestedFps: 60,
            queueAgeUs: 10_000,
            latencyBudgetUs: 100_000,
            recoveryActive: false,
            rebindInFlight: false
        )
        for _ in 0..<(AdaptiveResolutionPolicy.upshiftWindows - 1) {
            precondition(policy.observe(nowMs: 10_000, observation: healthy) == .keep)
        }
        precondition(policy.observe(nowMs: 11_000, observation: healthy) == .upshift(source))

        var slowOutputPolicy = AdaptiveResolutionPolicy(sourceTarget: source)
        slowOutputPolicy.record(decision: .downshift(fallback), success: true, nowMs: 0)
        let slowOutput = AdaptiveResolutionObservation(
            receiverLossDelta: 0,
            encodedFps: 30,
            transmittedFps: 60,
            requestedFps: 60,
            queueAgeUs: 10_000,
            latencyBudgetUs: 100_000,
            recoveryActive: false,
            rebindInFlight: false
        )
        for nowMs: UInt64 in [5_000, 6_000, 7_000, 8_000, 9_000] {
            precondition(slowOutputPolicy.observe(nowMs: nowMs, observation: slowOutput) == .keep)
        }

        // ABR interaction: a bitrate change and a resolution change must be
        // distinguishable so Host status can record the transition reason.
        precondition(
            AdaptiveResolutionTransitionKind(
                previousActive: source,
                nextActive: fallback
            ) == .resolution_changed
        )
        precondition(
            AdaptiveResolutionTransitionKind(
                previousActive: fallback,
                nextActive: fallback
            ) == .bitrate_changed
        )

        // A congested 4K stream whose bitrate floor has already collapsed
        // must request the fallback instead of sinking further into a
        // low-FPS state, and a 1440p stream must keep its floor decision.
        let floor4K = adaptiveBitrateFloorDecision(
            activeWidth: 3_840,
            activeHeight: 2_160,
            floorBitrate: 24_000_000,
            currentBitrate: 25_000_000
        )
        guard case .downshiftTo1440p = floor4K else {
            fatalError("4K floor collapse must request the fallback")
        }
        precondition(
            adaptiveBitrateFloorDecision(
                activeWidth: 2_560,
                activeHeight: 1_440,
                floorBitrate: 8_000_000,
                currentBitrate: 9_000_000
            ) == .keepFloor
        )
        precondition(
            adaptiveBitrateFloorDecision(
                activeWidth: 3_840,
                activeHeight: 2_160,
                floorBitrate: 24_000_000,
                currentBitrate: 30_000_000
            ) == .keepFloor
        )
        precondition(
            adaptiveBitrateFloorDecision(
                activeWidth: 3_200,
                activeHeight: 2_000,
                floorBitrate: 24_000_000,
                currentBitrate: 25_000_000
            ) == .downshiftTo1440p
        )
    }
}
