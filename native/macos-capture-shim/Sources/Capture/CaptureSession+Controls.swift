import Foundation
import AppKit
import ScreenCaptureKit
import VideoToolbox
import CoreMedia
import CoreVideo
import CoreGraphics
import IOSurface
import Security
import Darwin
import OSLog

extension CaptureSession {
    func setInputEnabled(_ enabled: Bool) -> Bool {
        if enabled && !CGPreflightPostEventAccess() {
            return false
        }
        inputLock.lock()
        let changed = inputEnabled != enabled
        inputEnabled = enabled
        inputLock.unlock()
        if changed && !enabled {
            inputQueue.async { [weak self] in
                self?.releaseInjectedInput()
            }
        }
        if changed, sock >= 0 {
            inputQueue.async { [weak self] in
                guard let self, self.sock >= 0 else { return }
                self.sendInputStatus(fd: self.sock)
            }
        }
        return true
    }

    /// Apply an operator-selected quality cap to the live encoder. VideoToolbox
    /// may reject the quality property after the session is prepared, so the
    /// matching bitrate scale is applied in the same encode queue as the
    /// automatic controller. Passing nil releases the cap and resumes Auto.
    func setQualityOverride(_ quality: Float?) -> String? {
        let policy = encoderQualityPolicy(
            codec: codecKind,
            width: outWidth,
            height: outHeight
        )
        guard policy.initialHint != nil else {
            return "manual quality override is available for 4K video sessions only"
        }
        if let quality, !(0.25...0.5).contains(quality) {
            return "quality override must be between 0.25 and 0.50"
        }
        guard let compressionSession = session else {
            return "encoder session is not ready"
        }

        stateLock.lock()
        let isAdaptiveQp = appliedEncoderExperimentValue == .adaptiveQp
        stateLock.unlock()
        if isAdaptiveQp {
            stateLock.lock()
            let previousQp = adaptiveQpController.currentBaseFrameQp
            if let quality {
                let percent = Int32((Double(quality) * 100.0).rounded())
                guard let qp = baseFrameQp(forSliderPercent: percent) else {
                    stateLock.unlock()
                    return "adaptiveQp quality slider is outside 25...50"
                }
                _ = adaptiveQpController.applyManualSlider(percent: percent)
                manualQualityHint = quality
                currentQualityHint = quality
                qualityAdaptationLastStatus = "manual_base_frame_qp"
                if qp != previousQp { baseFrameQpChanges &+= 1 }
            } else {
                _ = adaptiveQpController.applyManualSlider(percent: 0)
                manualQualityHint = nil
                currentQualityHint = nil
                qualityAdaptationLastStatus = "auto_base_frame_qp"
                if adaptiveQpController.currentBaseFrameQp != previousQp {
                    baseFrameQpChanges &+= 1
                }
            }
            stateLock.unlock()
            return nil
        }

        stateLock.lock()
        let current = currentQualityHint ?? 0.5
        let currentBitrate = max(1, currentAverageBitrate)
        let canApplyGenericQuality = encoderAppliedProperties.contains(
            EncoderOptionalProperty.quality.rawValue
        )
        stateLock.unlock()

        guard let quality else {
            stateLock.lock()
            manualQualityHint = nil
            qualityAdaptationLastStatus = "auto"
            stateLock.unlock()
            return nil
        }

        let targetBitrate = max(
            1_000_000,
            Int(
                (Double(currentBitrate)
                    * adaptiveQualityBitrateScale(qualityHint: quality)
                    / adaptiveQualityBitrateScale(qualityHint: current)).rounded()
            )
        )
        let (qualityStatus, bitrateStatus) = encodeQueue.sync {
            let qualityStatus: OSStatus
            if canApplyGenericQuality {
                qualityStatus = VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_Quality,
                    value: quality as CFNumber
                )
            } else {
                qualityStatus = -1
            }
            var bitrateStatus: OSStatus = noErr
            if targetBitrate != currentBitrate {
                bitrateStatus = VTSessionSetProperty(
                    compressionSession,
                    key: kVTCompressionPropertyKey_AverageBitRate,
                    value: targetBitrate as CFNumber
                )
                if bitrateStatus == noErr {
                    let hardLimitBytes = max(1, Int(Double(targetBitrate) / 8.0 * 1.25))
                    _ = VTSessionSetProperty(
                        compressionSession,
                        key: kVTCompressionPropertyKey_DataRateLimits,
                        value: [hardLimitBytes, 1] as CFArray
                    )
                }
            }
            return (qualityStatus, bitrateStatus)
        }

        guard qualityStatus == noErr || bitrateStatus == noErr else {
            return "quality override rejected by the encoder (quality=\(qualityStatus), bitrate=\(bitrateStatus))"
        }

        stateLock.lock()
        manualQualityHint = quality
        currentQualityHint = quality
        if bitrateStatus == noErr {
            currentAverageBitrate = targetBitrate
        }
        qualityAdaptationChanges &+= 1
        qualityAdaptationLastStatus = qualityStatus == noErr
            ? "manual"
            : "manual_bitrate_fallback"
        if qualityStatus != noErr {
            qualityAdaptationRejections &+= 1
        }
        stateLock.unlock()
        return nil
    }

     func startInputReceiver(fd: Int32) {
        inputQueue.sync {
            guard inputReadSource == nil else { return }
            let source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: inputQueue)
            source.setEventHandler { [weak self] in
                self?.consumeViewerControl(fd)
            }
            inputReadSource = source
            source.resume()
        }
        // Defensive token rebinding, not a live reconnect path: sessions own
        // one socket for their whole lifetime and startInputReceiver runs
        // once. The LCD1 coordinator stays silent until a token is installed
        // and embeds it in every packet, so if a future reconnect ever
        // replaces the session token, a live cursor stream rebinds here
        // instead of streaming packets the viewer would reject.
        cursorLock.lock()
        let hadCoordinator = cursorCoordinator != nil
        cursorLock.unlock()
        if hadCoordinator {
            cursorLock.lock()
            cursorCoordinator?.setToken(viewerControlToken)
            cursorLock.unlock()
        }
    }

     func stopInputReceiver() {
        inputQueue.sync {
            inputReadSource?.cancel()
            inputReadSource = nil
        }
    }

    static let tccDeniedHint = "screen-recording permission required (System Settings > Privacy & Security > Screen Recording)"
}

