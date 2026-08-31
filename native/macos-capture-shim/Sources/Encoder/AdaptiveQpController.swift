import Foundation

func adaptiveQpWindowIsPressured(
    encoderDrops: Int64,
    validOutputFps: UInt32,
    submitP95Us: UInt64
) -> Bool {
    encoderDrops > 0 || validOutputFps < 55 || submitP95Us > 16_667
}

func adaptiveQpStableWindowIsReady(
    encoderDrops: Int64,
    validOutputFps: UInt32,
    callbackP95Us: UInt64,
    networkOldestAgeUs: UInt64
) -> Bool {
    encoderDrops == 0
        && validOutputFps >= 59
        && callbackP95Us <= 18_500
        && networkOldestAgeUs <= 16_667
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
        networkOldestAgeUs: UInt64
    ) -> Int32 {
        guard manualBaseFrameQp == nil else { return currentBaseFrameQp }
        let pressured = adaptiveQpWindowIsPressured(
            encoderDrops: encoderDrops,
            validOutputFps: validOutputFps,
            submitP95Us: submitP95Us
        )
        if pressured {
            stableWindows = 0
        } else if adaptiveQpStableWindowIsReady(
            encoderDrops: encoderDrops,
            validOutputFps: validOutputFps,
            callbackP95Us: callbackP95Us,
            networkOldestAgeUs: networkOldestAgeUs
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
