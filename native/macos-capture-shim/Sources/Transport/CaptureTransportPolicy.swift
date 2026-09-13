import Foundation

enum UdpStabilityProfile: String, Equatable {
    case legacy
    case auto
    case responsive
    case balanced
    case stable
    case custom
}

struct AppliedUdpStability: Equatable {
    let profile: UdpStabilityProfile
    let burstDatagrams: Int
    let fecParityShards: Int
    let adaptivePacing: Bool

    static let legacy = AppliedUdpStability(
        profile: .legacy,
        burstDatagrams: 8,
        fecParityShards: 2,
        adaptivePacing: false
    )

    static func validated(
        profileName: String?,
        burstDatagrams: Int,
        fecParityShards: Int,
        adaptivePacing: Bool
    ) -> AppliedUdpStability? {
        guard let profileName,
              let profile = UdpStabilityProfile(rawValue: profileName),
              profile != .legacy,
              [2, 4, 8, 16].contains(burstDatagrams),
              [2, 4].contains(fecParityShards) else {
            return nil
        }
        if adaptivePacing && profile != .auto && profile != .custom {
            return nil
        }
        let applied = AppliedUdpStability(
            profile: profile,
            burstDatagrams: burstDatagrams,
            fecParityShards: fecParityShards,
            adaptivePacing: adaptivePacing
        )
        let canonical: Bool
        switch profile {
        case .auto:
            canonical = applied == .init(
                profile: .auto,
                burstDatagrams: 4,
                fecParityShards: 2,
                adaptivePacing: true
            )
        case .responsive:
            canonical = applied == .init(
                profile: .responsive,
                burstDatagrams: 8,
                fecParityShards: 2,
                adaptivePacing: false
            )
        case .balanced:
            canonical = applied == .init(
                profile: .balanced,
                burstDatagrams: 4,
                fecParityShards: 2,
                adaptivePacing: false
            )
        case .stable:
            canonical = applied == .init(
                profile: .stable,
                burstDatagrams: 2,
                fecParityShards: 4,
                adaptivePacing: false
            )
        case .custom:
            canonical = true
        case .legacy:
            canonical = false
        }
        return canonical ? applied : nil
    }
}

/// Both sealed start exports use the same decision. Non-UDP start callers pass
/// a canonical resolved UDP tuple; reconfigure callers use auto/0/0/false when
/// there is no UDP session policy. Neither tuple enables UDP pacing over TCP.
func captureTransportPolicy(
    transport: MediaTransportKind, profileName: String?, burstDatagrams: Int,
    fecParityShards: Int, adaptivePacing: Bool
) -> AppliedUdpStability? {
    let validated = AppliedUdpStability.validated(
        profileName: profileName, burstDatagrams: burstDatagrams,
        fecParityShards: fecParityShards, adaptivePacing: adaptivePacing
    )
    if transport == .udp { return validated }
    let placeholder = profileName == "auto" && burstDatagrams == 0
        && fecParityShards == 0 && !adaptivePacing
    guard placeholder || validated != nil else { return nil }
    return .legacy
}
