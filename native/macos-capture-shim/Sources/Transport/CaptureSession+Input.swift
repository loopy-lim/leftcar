import Foundation
import AppKit
import CoreGraphics

// Mouse events post at the session tap: macOS 26 drops synthesized button
// and scroll state injected at the HID tap (moves still warp the cursor
// there, which made the regression look like dead clicks). Keyboard events
// keep the HID tap, which continues to deliver.
extension CaptureSession {
    /// Authenticated state packet for the viewer's lock indicator. It is sent
    /// after the UDP proof and whenever the per-session opt-in changes. ACKs
    /// carry the same bit so a later click also repairs a lost status packet.
     func sendInputStatus(fd: Int32) {
        inputLock.lock()
        let enabled = inputEnabled
        inputLock.unlock()
        var status = Data("LCS1".utf8)
        status.append(enabled ? 1 : 0)
        status.append(viewerControlToken)
        _ = sendControlPayload(status, fd: fd)
    }

     func handleInputMessage(
        _ message: Data,
        fd: Int32,
        destination: sockaddr_in?
    ) {
        guard message.count >= 10,
              message.prefix(4) == Data("LCI1".utf8) else {
            return
        }
        let sequence = readUInt32BE(message, at: 4)
        let kind = message[8]
        let reliable = message[9] & 1 == 1

        if !reliable {
            guard kind == 1, message.count == 18 else { return }
            inputLock.lock()
            let newer = Int32(bitPattern: sequence &- lastPointerInputSequence) > 0
            if newer {
                lastPointerInputSequence = sequence
            }
            let enabled = inputEnabled
            inputLock.unlock()
            if newer && enabled {
                injectPointerMove(message)
            }
            return
        }

        inputLock.lock()
        let last = lastReliableInputSequence
        let enabled = inputEnabled
        inputLock.unlock()
        if sequence == last {
            sendInputAck(sequence: sequence, fd: fd, destination: destination)
            return
        }
        // Release-all is the fail-safe resynchronization packet. It may skip
        // a lost reliable transition, but an older delayed release must never
        // cancel newer input.
        if kind == 5 {
            guard message.count == 10,
                  Int32(bitPattern: sequence &- last) > 0 else {
                return
            }
            if enabled { releaseInjectedInput() }
            inputLock.lock()
            lastReliableInputSequence = sequence
            inputLock.unlock()
            sendInputAck(sequence: sequence, fd: fd, destination: destination)
            return
        }
        guard sequence == last &+ 1,
              validateAndInjectReliableInput(message, kind: kind, enabled: enabled) else {
            return
        }
        inputLock.lock()
        lastReliableInputSequence = sequence
        inputLock.unlock()
        sendInputAck(sequence: sequence, fd: fd, destination: destination)
    }

     func validateAndInjectReliableInput(
        _ message: Data,
        kind: UInt8,
        enabled: Bool
    ) -> Bool {
        switch kind {
        case 2:
            guard message.count == 20 else { return false }
            if enabled { injectPointerButton(message) }
        case 3:
            guard message.count == 18 else { return false }
            if enabled { injectScroll(message) }
        case 4:
            guard message.count == 21 else { return false }
            if enabled { injectKey(message) }
        case 5:
            guard message.count == 10 else { return false }
            if enabled { releaseInjectedInput() }
        case 6:
            // Variable-length UTF-8 payload after the 10-byte header.
            guard message.count > 10 else { return false }
            if enabled { injectText(message) }
        default:
            return false
        }
        return true
    }

     func pointerPosition(x: UInt16, y: UInt16) -> CGPoint? {
        inputLock.lock()
        let bounds = inputBounds
        inputLock.unlock()
        guard let bounds else { return nil }
        let px = bounds.origin.x + CGFloat(x) / CGFloat(UInt16.max) * bounds.width
        let py = bounds.origin.y + CGFloat(y) / CGFloat(UInt16.max) * bounds.height
        return CGPoint(x: px, y: py)
    }

     func injectPointerMove(_ message: Data) {
        guard let point = pointerPosition(
            x: readUInt16BE(message, at: 10),
            y: readUInt16BE(message, at: 12)
        ) else { return }
        let buttons = readUInt32BE(message, at: 14)
        let type: CGEventType
        let button: CGMouseButton
        if buttons & 1 != 0 {
            type = .leftMouseDragged
            button = .left
        } else if buttons & 2 != 0 {
            type = .rightMouseDragged
            button = .right
        } else if buttons & 4 != 0 {
            type = .otherMouseDragged
            button = .center
        } else {
            type = .mouseMoved
            button = .left
        }
        lastPointerPosition = point
        CGEvent(
            mouseEventSource: nil,
            mouseType: type,
            mouseCursorPosition: point,
            mouseButton: button
        )?.post(tap: .cgSessionEventTap)
    }

     func mouseButton(mask: UInt8) -> CGMouseButton? {
        switch mask {
        case 1: return .left
        case 2: return .right
        case 4: return .center
        default: return nil
        }
    }

     func injectPointerButton(_ message: Data) {
        guard let point = pointerPosition(
            x: readUInt16BE(message, at: 10),
            y: readUInt16BE(message, at: 12)
        ) else { return }
        guard let button = mouseButton(mask: message[14]) else { return }
        let down = message[15] != 0
        let type: CGEventType
        switch (button, down) {
        case (.left, true): type = .leftMouseDown
        case (.left, false): type = .leftMouseUp
        case (.right, true): type = .rightMouseDown
        case (.right, false): type = .rightMouseUp
        case (_, true): type = .otherMouseDown
        case (_, false): type = .otherMouseUp
        }
        lastPointerPosition = point
        if down {
            pressedButtons.insert(button)
        } else {
            pressedButtons.remove(button)
        }
        CGEvent(
            mouseEventSource: nil,
            mouseType: type,
            mouseCursorPosition: point,
            mouseButton: button
        )?.post(tap: .cgSessionEventTap)
    }

     func injectScroll(_ message: Data) {
        horizontalScrollRemainder &+= Int32(bitPattern: readUInt32BE(message, at: 10))
        verticalScrollRemainder &+= Int32(bitPattern: readUInt32BE(message, at: 14))
        let horizontal = horizontalScrollRemainder / 1_000
        let vertical = verticalScrollRemainder / 1_000
        horizontalScrollRemainder %= 1_000
        verticalScrollRemainder %= 1_000
        guard horizontal != 0 || vertical != 0 else { return }
        // The viewer speaks content-tracks-fingers (natural scrolling). macOS
        // applies the system's natural-scroll flip to synthesized wheel
        // events too, so honor the user's direction preference here instead
        // of on every viewer.
        let direction: Int32 = usesNaturalScrolling ? -1 : 1
        CGEvent(
            scrollWheelEvent2Source: nil,
            units: .line,
            wheelCount: 2,
            wheel1: direction * vertical,
            wheel2: direction * horizontal,
            wheel3: 0
        )?.post(tap: .cgSessionEventTap)
    }

    /// Global scroll direction preference. Missing key means the macOS
    /// default: natural scrolling on.
    var usesNaturalScrolling: Bool {
        let domain = UserDefaults.standard.persistentDomain(forName: UserDefaults.globalDomain)
        if let value = domain?["com.apple.swipescrolldirection"] as? Bool {
            return value
        }
        return true
    }

    /// User-level remote key remap, e.g. Caps Lock → F17 for personal
    /// layouts: `defaults write NSGlobalDomain dev.leftcar.remoteKeyRemap
    /// -dict 115 240`. Synthetic CGEvents bypass Karabiner/hidutil device
    /// remaps, so this lookup is the one place a viewer key can be re-bound
    /// on the host side. Missing/invalid entries are ignored.
    var remoteKeyRemap: [UInt16: CGKeyCode] {
        let domain = UserDefaults.standard.persistentDomain(forName: UserDefaults.globalDomain)
        guard let table = domain?["dev.leftcar.remoteKeyRemap"] as? [String: Any] else {
            return [:]
        }
        var remap: [UInt16: CGKeyCode] = [:]
        for (rawKey, rawValue) in table {
            guard let key = UInt16(rawKey),
                  let target = remapTarget(rawValue) else { continue }
            remap[key] = target
        }
        return remap
    }

    private func remapTarget(_ raw: Any) -> CGKeyCode? {
        let number: NSNumber?
        if let n = raw as? NSNumber {
            number = n
        } else if let s = raw as? String {
            number = NumberFormatter().number(from: s)
        } else {
            number = nil
        }
        guard let n = number else { return nil }
        return CGKeyCode(exactly: n)
    }

     func keyboardFlags(metaState: UInt32) -> CGEventFlags {
        var flags: CGEventFlags = []
        if metaState & 0x0000_0001 != 0 { flags.insert(.maskShift) }
        if metaState & 0x0000_0002 != 0 { flags.insert(.maskAlternate) }
        if metaState & 0x0000_1000 != 0 { flags.insert(.maskControl) }
        if metaState & 0x0001_0000 != 0 { flags.insert(.maskCommand) }
        if metaState & 0x0010_0000 != 0 { flags.insert(.maskAlphaShift) }
        return flags
    }

     func macKeyCode(android code: UInt16) -> CGKeyCode? {
        let letters: [CGKeyCode] = [
            0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46,
            45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6,
        ]
        if (29...54).contains(code) { return letters[Int(code - 29)] }
        let digits: [CGKeyCode] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25]
        if (7...16).contains(code) { return digits[Int(code - 7)] }
        let keypad: [CGKeyCode] = [82, 83, 84, 85, 86, 87, 88, 89, 91, 92]
        if (144...153).contains(code) { return keypad[Int(code - 144)] }
        let functionKeys: [CGKeyCode] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111]
        if (131...142).contains(code) { return functionKeys[Int(code - 131)] }
        return [
            19: 126, 20: 125, 21: 123, 22: 124,
            55: 43, 56: 47, 57: 58, 58: 61, 59: 56, 60: 60,
            61: 48, 62: 49, 66: 36, 67: 51, 68: 50, 69: 27,
            70: 24, 71: 33, 72: 30, 73: 42, 74: 41, 75: 39,
            76: 44, 92: 116, 93: 121, 111: 53, 112: 117,
            113: 59, 114: 62, 115: 57, 117: 55, 118: 54,
            122: 115, 123: 119, 124: 114,
            154: 75, 155: 67, 156: 78, 157: 69, 158: 65,
            160: 76, 161: 81, 204: 104,
        ][code]
    }

     func injectKey(_ message: Data) {
        let androidCode = readUInt16BE(message, at: 10)
        // 사용자 리맵이 우선한다(예: Caps Lock 115 → F17 240); 없으면 기본
        // 안드로이드→Mac 키코드 표를 따른다.
        guard let keyCode = remoteKeyRemap[androidCode] ?? macKeyCode(android: androidCode) else { return }
        let metaState = readUInt32BE(message, at: 14)
        let down = message[18] != 0
        let repeatCount = readUInt16BE(message, at: 19)
        guard let event = CGEvent(
            keyboardEventSource: nil,
            virtualKey: keyCode,
            keyDown: down
        ) else { return }
        event.flags = keyboardFlags(metaState: metaState)
        if repeatCount > 0 {
            event.setIntegerValueField(.keyboardEventAutorepeat, value: 1)
        }
        if down {
            pressedKeys.insert(keyCode)
        } else {
            pressedKeys.remove(keyCode)
        }
        event.post(tap: .cghidEventTap)
    }

    /// Committed IME text from the viewer. Types the exact string instead of
    /// synthesizing hardware keys, which is what makes composed Hangul (and
    /// any other layout-independent text) reach the Mac correctly. Keyboard
    /// injection keeps the HID tap, which continues to deliver.
    func injectText(_ message: Data) {
        guard let text = String(data: message.dropFirst(10), encoding: .utf8),
              !text.isEmpty else {
            return
        }
        for character in text {
            var units = Array(String(character).utf16)
            units.withUnsafeMutableBufferPointer { buffer in
                guard let base = buffer.baseAddress else { return }
                let down = CGEvent(
                    keyboardEventSource: nil,
                    virtualKey: 0,
                    keyDown: true
                )
                let up = CGEvent(
                    keyboardEventSource: nil,
                    virtualKey: 0,
                    keyDown: false
                )
                down?.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: base)
                up?.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: base)
                down?.post(tap: .cghidEventTap)
                up?.post(tap: .cghidEventTap)
            }
        }
    }

     func releaseInjectedInput() {
        for keyCode in pressedKeys {
            CGEvent(
                keyboardEventSource: nil,
                virtualKey: keyCode,
                keyDown: false
            )?.post(tap: .cghidEventTap)
        }
        pressedKeys.removeAll(keepingCapacity: true)
        for button in pressedButtons {
            let type: CGEventType = button == .left
                ? .leftMouseUp
                : (button == .right ? .rightMouseUp : .otherMouseUp)
            CGEvent(
                mouseEventSource: nil,
                mouseType: type,
                mouseCursorPosition: lastPointerPosition,
                mouseButton: button
            )?.post(tap: .cgSessionEventTap)
        }
        pressedButtons.removeAll(keepingCapacity: true)
        horizontalScrollRemainder = 0
        verticalScrollRemainder = 0
    }

}
