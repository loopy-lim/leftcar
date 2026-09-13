import Foundation

 enum CaptureBackendKind: String {
    case screenCaptureKit
    case cgDisplayStream

    static func parse(_ value: String?) -> CaptureBackendKind? {
        guard let value else { return .screenCaptureKit }
        switch value.lowercased() {
        case "sck", "screencapturekit": return .screenCaptureKit
        case "cg", "cgdisplaystream": return .cgDisplayStream
        default: return nil
        }
    }
}

 enum MediaTransportKind: String {
    case udp
    case tcp
    case usb
    case adbTcp

    var usesTCP: Bool {
        self == .tcp || self == .usb || self == .adbTcp
    }

    static func parse(_ value: String?) -> MediaTransportKind? {
        guard let value else { return .udp }
        switch value.lowercased() {
        case "udp": return .udp
        case "tcp", "wifitcp", "wifi-tcp": return .tcp
        case "usb", "aoap": return .usb
        case "adbtcp", "adb-tcp": return .adbTcp
        default: return nil
        }
    }
}

 enum StreamContentMode: String {
    case interactive
    case video

    static func parse(_ value: String?) -> StreamContentMode? {
        guard let value else { return .interactive }
        switch value.lowercased() {
        case "interactive", "latency": return .interactive
        case "video", "movie": return .video
        default: return nil
        }
    }
}

