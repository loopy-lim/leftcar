import Foundation

func splitDiagnosticEnabled(
    environment: [String: String] = ProcessInfo.processInfo.environment
) -> Bool {
    environment["LEFTCAR_ENABLE_SPLIT_DIAGNOSTIC"] == "1"
}

enum SplitEncoderStrategy: String, Equatable {
    case dualAve
    case dualRtvc
    case mirrorLeft

    static func parse(
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> SplitEncoderStrategy {
        switch environment["LEFTCAR_SPLIT_ENCODER_DIAGNOSTIC_MODE"] {
        case dualRtvc.rawValue: return .dualRtvc
        case mirrorLeft.rawValue: return .mirrorLeft
        default: return .dualAve
        }
    }

    var tileBackend: SplitTileEncoderBackend {
        self == .dualAve ? .ave : .rtvc
    }

    var usesIndependentRightEncoder: Bool {
        self != .mirrorLeft
    }

    var encoderMode: String {
        switch self {
        case .dualAve: return "splitVertical"
        case .dualRtvc: return "splitVertical-dualRtvc-diagnostic"
        case .mirrorLeft: return "splitVertical-mirrorLeft-diagnostic"
        }
    }

    var isDiagnostic: Bool {
        self != .dualAve
    }
}
