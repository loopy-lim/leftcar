import Foundation

/// 60fps 기준 상수(프레임 예산 16_667µs 등)를 타겟 fps 스케일로 일반화한다.
/// 정수 연산을 고정해 60에서는 기존 상수와 정확히 같은 경계가 나온다
/// (EncodePolicyTests가 16_667/18_500 경계를 핀한다).
func adaptiveQpFrameBudgetUs(targetFps: UInt32) -> UInt64 {
    precondition(targetFps > 0)
    return (1_000_000 + UInt64(targetFps) - 1) / UInt64(targetFps)
}

func adaptiveQpWindowIsPressured(
    encoderDrops: Int64,
    validOutputFps: UInt32,
    submitP95Us: UInt64,
    targetFps: UInt32
) -> Bool {
    encoderDrops > 0
        || validOutputFps < (targetFps * 55) / 60
        || submitP95Us > adaptiveQpFrameBudgetUs(targetFps: targetFps)
}

func adaptiveQpStableWindowIsReady(
    encoderDrops: Int64,
    validOutputFps: UInt32,
    callbackP95Us: UInt64,
    networkOldestAgeUs: UInt64,
    targetFps: UInt32
) -> Bool {
    let budgetUs = adaptiveQpFrameBudgetUs(targetFps: targetFps)
    return encoderDrops == 0
        && validOutputFps >= (targetFps * 59) / 60
        && callbackP95Us <= (budgetUs * 111) / 100
        && networkOldestAgeUs <= budgetUs
}

struct AdaptiveQpController: Equatable {
    private(set) var automaticBaseFrameQp: Int32 = 32
    private(set) var stableWindows = 0
     var manualBaseFrameQp: Int32?

    var currentBaseFrameQp: Int32 {
        manualBaseFrameQp ?? automaticBaseFrameQp
    }

    @discardableResult
    mutating func observeWindow(
        encoderDrops: Int64,
        validOutputFps: UInt32,
        submitP95Us: UInt64,
        callbackP95Us: UInt64,
        networkOldestAgeUs: UInt64,
        targetFps: UInt32
    ) -> Int32 {
        guard manualBaseFrameQp == nil else { return currentBaseFrameQp }
        let pressured = adaptiveQpWindowIsPressured(
            encoderDrops: encoderDrops,
            validOutputFps: validOutputFps,
            submitP95Us: submitP95Us,
            targetFps: targetFps
        )
        if pressured {
            stableWindows = 0
        } else if adaptiveQpStableWindowIsReady(
            encoderDrops: encoderDrops,
            validOutputFps: validOutputFps,
            callbackP95Us: callbackP95Us,
            networkOldestAgeUs: networkOldestAgeUs,
            targetFps: targetFps
        ) {
            stableWindows += 1
        } else {
            stableWindows = 0
        }
        automaticBaseFrameQp = nextAdaptiveBaseFrameQp(
            current: automaticBaseFrameQp,
            pressured: pressured,
            stableWindows: stableWindows
        )
        return automaticBaseFrameQp
    }

    @discardableResult
    mutating func applyManualSlider(percent: Int32) -> Int32? {
        guard percent == 0 || (25...50).contains(percent) else { return nil }
        if percent == 0 {
            manualBaseFrameQp = nil
            return nil
        }
        let qp = baseFrameQp(forSliderPercent: percent) ?? automaticBaseFrameQp
        manualBaseFrameQp = qp
        return qp
    }
}
