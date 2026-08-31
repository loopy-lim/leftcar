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
    func applyOptionalEncoderProperties(
        to s: VTCompressionSession,
        policy: EncoderSessionPolicy,
        supportedPropertyKeys: Set<String>,
        latencyPolicy: EncoderLatencyPolicy,
        width w: Int32,
        height h: Int32,
        report: inout EncoderConfigurationReport
    ) {
            var supportedOptionalProperties = Set<EncoderOptionalProperty>()
            if #available(macOS 11.0, *), supportedPropertyKeys.contains(
                kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality as String
            ) {
                supportedOptionalProperties.insert(.prioritizeSpeed)
            }
            if #available(macOS 15.0, *) {
                if supportedPropertyKeys.contains(
                    kVTCompressionPropertyKey_MaximumRealTimeFrameRate as String
                ) {
                    supportedOptionalProperties.insert(.maximumRealTimeFrameRate)
                }
                if supportedPropertyKeys.contains(
                    kVTCompressionPropertyKey_SuggestedLookAheadFrameCount as String
                ) {
                    supportedOptionalProperties.insert(.suggestedLookAheadFrameCount)
                }
            }
            if supportedPropertyKeys.contains(
                kVTCompressionPropertyKey_MaximizePowerEfficiency as String
            ) {
                supportedOptionalProperties.insert(.maximizePowerEfficiency)
            }
            if supportedPropertyKeys.contains(kVTCompressionPropertyKey_Quality as String) {
                supportedOptionalProperties.insert(.quality)
            }

            if policy.mode == .ave {
                let optionalPlan = optionalPropertyPlan(
                    mode: policy.mode,
                    supported: supportedOptionalProperties
                )
                for property in EncoderOptionalProperty.allCases where !optionalPlan.contains(property) {
                    if supportedOptionalProperties.contains(property) {
                        report.recordSuppressed(property.rawValue)
                    } else {
                        report.recordUnsupported(property.rawValue)
                    }
                }
                for property in optionalPlan {
                    let propertyStatus: OSStatus
                    switch property {
                    case .prioritizeSpeed:
                        if #available(macOS 11.0, *) {
                            propertyStatus = VTSessionSetProperty(
                                s,
                                key: kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality,
                                value: true as CFBoolean
                            )
                        } else {
                            propertyStatus = kVTPropertyNotSupportedErr
                        }
                    case .maximumRealTimeFrameRate:
                        if #available(macOS 15.0, *) {
                            propertyStatus = VTSessionSetProperty(
                                s,
                                key: kVTCompressionPropertyKey_MaximumRealTimeFrameRate,
                                value: Int32(latencyPolicy.maximumRealTimeFrameRate) as CFNumber
                            )
                        } else {
                            propertyStatus = kVTPropertyNotSupportedErr
                        }
                    case .suggestedLookAheadFrameCount:
                        if #available(macOS 15.0, *) {
                            propertyStatus = VTSessionSetProperty(
                                s,
                                key: kVTCompressionPropertyKey_SuggestedLookAheadFrameCount,
                                value: 0 as CFNumber
                            )
                        } else {
                            propertyStatus = kVTPropertyNotSupportedErr
                        }
                    case .maximizePowerEfficiency:
                        propertyStatus = VTSessionSetProperty(
                            s,
                            key: kVTCompressionPropertyKey_MaximizePowerEfficiency,
                            value: false as CFBoolean
                        )
                    case .quality:
                        let quality = initialEncoderQuality(
                            mode: policy.mode,
                            width: UInt32(w),
                            height: UInt32(h)
                        ) ?? 0.25
                        propertyStatus = VTSessionSetProperty(
                            s,
                            key: kVTCompressionPropertyKey_Quality,
                            value: quality as CFNumber
                        )
                    }
                    if propertyStatus == noErr {
                        report.recordApplied(property.rawValue)
                    } else {
                        report.recordRejected(property.rawValue, status: propertyStatus)
                    }
                }
            }
    }
}

