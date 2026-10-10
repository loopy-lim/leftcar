import Foundation
import CoreGraphics

let kmSyntheticTag: Int64 = 0x4c434b4d

enum KmNativeOwner: Int32 { case local = 0, macToAndroid = 1, androidToMac = 2 }

/// Pure policy used before every native tap suppression decision.
struct KmEventPolicy {
    var owner: KmNativeOwner = .local
    func isEscape(key: UInt16, flags: CGEventFlags) -> Bool {
        key == 53 && flags.contains([.maskControl, .maskAlternate, .maskCommand])
    }
    func suppress(tag: Int64) -> Bool { owner == .macToAndroid && tag != kmSyntheticTag }
    func nearSharedEdge(point: CGPoint, bounds: CGRect, left: Bool) -> Bool {
        guard !bounds.isEmpty, point.x.isFinite, point.y.isFinite,
              point.y >= bounds.minY, point.y < bounds.maxY else { return false }
        return left ? point.x <= bounds.minX + 2 : point.x >= bounds.maxX - 3
    }
}

/// USB keyboard usage IDs are the wire namespace, including modifiers E0..E7.
enum KmHidKeyMap {
    static let androidToHid: [UInt16: UInt16] = {
        var map: [UInt16: UInt16] = [:]
        for index in 0..<26 { map[UInt16(29 + index)] = UInt16(4 + index) }
        for index in 1...9 { map[UInt16(7 + index)] = UInt16(29 + index) }
        map[7] = 39
        let pairs: [(UInt16, UInt16)] = [(66,40),(111,41),(67,42),(61,43),(62,44),
          (69,45),(70,46),(71,47),(72,48),(73,49),(74,51),(75,52),(68,53),
          (55,54),(56,55),(76,56),(115,57),(116,71),(120,70),(121,72),
          (122,74),(123,77),(124,73),(112,76),(92,75),(93,78),
          (22,79),(21,80),(20,81),(19,82),(113,224),(59,225),(57,226),(117,227),
          (114,228),(60,229),(58,230),(118,231)]
        for (android,hid) in pairs { map[android] = hid }
        for index in 0..<12 { map[UInt16(131 + index)] = UInt16(58 + index) }
        return map
    }()
    static let hidToMac: [UInt16: UInt16] = {
        var map: [UInt16: UInt16] = [:]
        for (android,hid) in androidToHid { if let mac = AndroidKeyMap.mac[android] { map[hid] = mac } }
        return map
    }()
    static let macToHid: [UInt16: UInt16] = {
        var map: [UInt16: UInt16] = [:]
        for (hid,mac) in hidToMac { map[mac] = hid }
        return map
    }()
}

/// Track physical held state even while Armed, so edge transfer cannot strand local key ups.
struct KmPhysicalState {
    var keys = Set<UInt16>()
    var buttons = Set<Int>()
    var isHeld: Bool { !keys.isEmpty || !buttons.isEmpty }
    mutating func key(_ code: UInt16, down: Bool) { if down { keys.insert(code) } else { keys.remove(code) } }
    mutating func button(_ code: Int, down: Bool) { if down { buttons.insert(code) } else { buttons.remove(code) } }
}
