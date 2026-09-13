import Foundation

enum EncoderExperiment: String, Equatable {
    case auto
    case rateControl
    case adaptiveQp
    case encoderPool
    case splitHorizontal
    case splitVertical

    static func parse(_ raw: String?) -> EncoderExperiment? {
        guard let raw else { return .auto }
        guard let value = EncoderExperiment(rawValue: raw) else { return nil }
        switch value {
        case .auto, .rateControl, .adaptiveQp, .encoderPool, .splitVertical:
            return value
        case .splitHorizontal:
            return nil
        }
    }
}

enum EncoderExperimentParseResult: Equatable {
    case success(EncoderExperiment)
    case failure(String)
}

func parseEncoderExperimentName(_ raw: String?) -> EncoderExperimentParseResult {
    guard let raw else { return .success(.auto) }
    guard let experiment = EncoderExperiment.parse(raw) else {
        return .failure("unknown encoder experiment: \(raw)")
    }
    return .success(experiment)
}

func parseEncoderExperimentCString(
    _ raw: UnsafePointer<CChar>?
) -> EncoderExperimentParseResult {
    guard let raw else { return .success(.auto) }
    guard let decoded = String(validatingUTF8: raw) else {
        return .failure("unknown encoder experiment: invalid UTF-8")
    }
    return parseEncoderExperimentName(decoded)
}

