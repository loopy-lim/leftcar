import Foundation
import AppKit
import CoreGraphics
import Carbon

/// Independent OS input lifetime. It never instantiates CaptureSession or asks for screen access.
final class KmNativeSession {
    typealias Sink = @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<CChar>?) -> Void
    private let sink: Sink
    private let context: UnsafeMutableRawPointer?
    private let state = NSLock()
    private let ownership = KmOwnershipGate()
    private let callbacks = KmCallbackFence()
    private var shutdownRequested = false
    private var emergencyPending = false
    private var pendingReleases = 0
    private var stopped = false
    private var tap: CFMachPort?
    private var loop: CFRunLoop?
    private let started = DispatchSemaphore(value: 0)
    private let finished = DispatchGroup()
    private let injection = KmWorkQueue()
    private var pressedKeys = Set<UInt16>()
    private var capturedModifiers = Set<UInt16>()
    private var pressedButtons = Set<Int>()
    private var wheelRemainderX: Double = 0
    private var wheelRemainderY: Double = 0
    private var observers: [NSObjectProtocol] = []
    private var thread: Thread?
    private let source = CGEventSource(stateID: .privateState)

    init(sink: @escaping Sink, context: UnsafeMutableRawPointer?) { self.sink = sink; self.context = context }

    static func lockedOrSecure() -> Bool {
        guard let session = CGSessionCopyCurrentDictionary() as? [String: Any] else { return true }
        return IsSecureEventInputEnabled() || (session["CGSSessionScreenIsLocked"] as? Bool == true)
    }
    static func preflight() -> Int32 {
        var flags: Int32 = 0
        if CGPreflightListenEventAccess() { flags |= 1 }
        if AXIsProcessTrusted() { flags |= 2 }
        if !lockedOrSecure() { flags |= 4 }
        return flags
    }
    static func desktopBounds() -> CGRect {
        var count: UInt32 = 0
        guard CGGetActiveDisplayList(0, nil, &count) == .success, count > 0 else { return .null }
        var displays = [CGDirectDisplayID](repeating: 0, count: Int(count))
        guard CGGetActiveDisplayList(count, &displays, &count) == .success else { return .null }
        return displays.prefix(Int(count)).reduce(CGRect.null) { $0.union(CGDisplayBounds($1)) }
    }
    func start() -> Bool {
        guard Self.preflight() == 7 else { return false }
        finished.enter()
        let worker = Thread { [self] in
            let eventTypes: [CGEventType] = [.mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged,
              .leftMouseDown, .leftMouseUp, .rightMouseDown, .rightMouseUp, .otherMouseDown, .otherMouseUp,
              .scrollWheel, .keyDown, .keyUp, .flagsChanged]
            let mask = eventTypes.reduce(CGEventMask(0)) { $0 | (CGEventMask(1) << $1.rawValue) }
            let callback: CGEventTapCallBack = { _, type, event, raw in
                guard let raw else { return Unmanaged.passUnretained(event) }
                return Unmanaged<KmNativeSession>.fromOpaque(raw).takeUnretainedValue().receive(type, event)
            }
            let newTap = CGEvent.tapCreate(tap: .cgSessionEventTap, place: .headInsertEventTap,
                options: .defaultTap, eventsOfInterest: mask, callback: callback,
                userInfo: Unmanaged.passUnretained(self).toOpaque())
            state.lock(); tap = newTap; loop = CFRunLoopGetCurrent(); state.unlock()
            guard let newTap, let runSource = CFMachPortCreateRunLoopSource(nil, newTap, 0) else {
                started.signal(); finished.leave(); return
            }
            CFRunLoopAddSource(CFRunLoopGetCurrent(), runSource, .commonModes)
            ownership.withEvent { _, physical in
                for code in UInt16(0)...UInt16(127) {
                    if CGEventSource.keyState(.combinedSessionState,key:code) { physical.key(code,down:true) }
                }
                for button in 0...7 {
                    if let value = CGMouseButton(rawValue:UInt32(button)), CGEventSource.buttonState(.combinedSessionState,button:value) { physical.button(button,down:true) }
                }
            }
            CGEvent.tapEnable(tap: newTap, enable: true)
            started.signal()
            while true {
                state.lock(); let shouldStop = stopped; state.unlock()
                if shouldStop { break }
                CFRunLoopRunInMode(.defaultMode, 0.1, true)
            }
            CGEvent.tapEnable(tap: newTap, enable: false)
            CFMachPortInvalidate(newTap)
            CFRunLoopRemoveSource(CFRunLoopGetCurrent(), runSource, .commonModes)
            finished.leave()
        }
        thread = worker; worker.name = "leftcar-km-tap"; worker.start()
        guard started.wait(timeout: .now() + 2) == .success else { stop(); return false }
        state.lock(); let ok = tap != nil; state.unlock()
        guard ok else { return false }
        observers.append(NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.willSleepNotification,
            object: nil, queue: nil) { [weak self] _ in self?.observeEmergency() })
        observers.append(NSWorkspace.shared.notificationCenter.addObserver(forName: NSWorkspace.sessionDidResignActiveNotification,
            object:nil, queue:nil) { [weak self] _ in self?.observeEmergency() })
        observers.append(DistributedNotificationCenter.default().addObserver(forName: NSNotification.Name("com.apple.screenIsLocked"),
            object: nil, queue: nil) { [weak self] _ in self?.observeEmergency() })
        return true
    }
    func emit(_ payload: [String: Any]) {
        callbacks.emit {
            guard let data = try? JSONSerialization.data(withJSONObject:payload),
                  let json = String(data:data,encoding:.utf8) else { return }
            json.withCString { sink(context,$0) }
        }
    }
    private func observeEmergency() { callbacks.emit { failClosed() } }
    func failClosed() {
        state.lock()
        guard !emergencyPending && !shutdownRequested else { state.unlock(); return }
        emergencyPending = true; state.unlock()
        // Never wait on injection from a tap/notification callback. Suppression remains
        // committed until the serial queue has actually posted all neutral input.
        injection.enqueue { [self] in
            releaseAllNow()
            _ = ownership.setOwner(.local)
            emit(["kind":"stop"])
        }
    }
    private func modifierFlag(_ hid: UInt16) -> CGEventFlags {
        switch hid {
        case 224,228: return .maskControl
        case 225,229: return .maskShift
        case 226,230: return .maskAlternate
        case 227,231: return .maskCommand
        case 57: return .maskAlphaShift
        default: return []
        }
    }
    private func mouseType(_ button:Int, down:Bool) -> CGEventType {
        switch button {
        case 0: return down ? .leftMouseDown : .leftMouseUp
        case 1: return down ? .rightMouseDown : .rightMouseUp
        default: return down ? .otherMouseDown : .otherMouseUp
        }
    }
    private func dragType(_ button:Int) -> CGEventType {
        switch button { case 0: return .leftMouseDragged; case 1: return .rightMouseDragged; default: return .otherMouseDragged }
    }
    func receive(_ type: CGEventType, _ event: CGEvent) -> Unmanaged<CGEvent>? {

        let tag = event.getIntegerValueField(.eventSourceUserData)
        if tag == kmSyntheticTag { return Unmanaged.passUnretained(event) }
        let code = UInt16(event.getIntegerValueField(.keyboardEventKeycode))
        return ownership.withEvent { current, physical in
        state.lock(); let isStopped = stopped; let stopping = shutdownRequested || emergencyPending; state.unlock()
        if isStopped { return Unmanaged.passUnretained(event) }
        if stopping { return current.suppress(tag:tag) ? nil : Unmanaged.passUnretained(event) }
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            failClosed(); return current.suppress(tag:tag) ? nil : Unmanaged.passUnretained(event)
        }
        if type == .keyDown && current.isEscape(key: code, flags: event.flags) {
            failClosed(); return nil
        }
        if Self.lockedOrSecure() { failClosed(); return current.suppress(tag:tag) ? nil : Unmanaged.passUnretained(event) }
        if type == .keyDown || type == .keyUp { physical.key(code,down:type == .keyDown) }
        if type == .flagsChanged {
            let hid = KmHidKeyMap.macToHid[code] ?? 0
            if hid != 57 { physical.key(code,down:event.flags.contains(modifierFlag(hid)) && !physical.keys.contains(code)) }
        }
        if [.leftMouseDown,.rightMouseDown,.otherMouseDown,.leftMouseUp,.rightMouseUp,.otherMouseUp].contains(type) {
            physical.button(Int(event.getIntegerValueField(.mouseEventButtonNumber)),down:[.leftMouseDown,.rightMouseDown,.otherMouseDown].contains(type))
        }
        let pointerTypes: Set<CGEventType> = [.mouseMoved, .leftMouseDragged, .rightMouseDragged, .otherMouseDragged]
        if pointerTypes.contains(type) {
            let point = event.location
            emit(["kind": "pointer", "x": point.x, "y": point.y,
                  "held":physical.isHeld, "injected":false, "dx": event.getDoubleValueField(.mouseEventDeltaX), "dy": event.getDoubleValueField(.mouseEventDeltaY)])
        }
        guard current.suppress(tag: tag) else { return Unmanaged.passUnretained(event) }
        switch type {
        case .leftMouseDown, .leftMouseUp, .rightMouseDown, .rightMouseUp, .otherMouseDown, .otherMouseUp:
            let button = event.getIntegerValueField(.mouseEventButtonNumber)
            emit(["kind":"button", "button":button,
                  "down": [.leftMouseDown,.rightMouseDown,.otherMouseDown].contains(type)])
        case .scrollWheel:
            emit(["kind":"wheel", "dx":event.getDoubleValueField(.scrollWheelEventDeltaAxis2),
                  "dy":event.getDoubleValueField(.scrollWheelEventDeltaAxis1)])
        case .keyDown, .keyUp, .flagsChanged:
            guard let hid = KmHidKeyMap.macToHid[code] else { failClosed(); return Unmanaged.passUnretained(event) }
            var down = type == .keyDown
            if type == .flagsChanged {
                if hid == 57 {
                    emit(["kind":"key", "code":hid, "down":true])
                    emit(["kind":"key", "code":hid, "down":false])
                    return nil
                }
                down = event.flags.contains(modifierFlag(hid)) && !capturedModifiers.contains(code)
                if down { capturedModifiers.insert(code) } else { capturedModifiers.remove(code) }
            }
            emit(["kind":"key", "code":hid, "down":down])
        default: break
        }
        return nil
        }
    }
    func setOwner(_ owner: KmNativeOwner) -> Bool {
        state.lock(); let unavailable = shutdownRequested || emergencyPending || pendingReleases > 0; state.unlock()
        guard !unavailable else { return false }
        return ownership.setOwner(owner,snapshotHeld: {
            for code in UInt16(0)...UInt16(127) {
                if CGEventSource.keyState(.combinedSessionState,key:code) { return true }
            }
            for button in 0...7 {
                if let value = CGMouseButton(rawValue:UInt32(button)), CGEventSource.buttonState(.combinedSessionState,button:value) { return true }
            }
            return false
        }, onCommit: { capturedModifiers.removeAll() })
    }
    private func post(_ event: CGEvent?, keyboard: Bool = false) {
        event?.setIntegerValueField(.eventSourceUserData, value: kmSyntheticTag)
        event?.flags = keyFlags()
        event?.post(tap: keyboard ? .cghidEventTap : .cgSessionEventTap)
    }
    private func keyFlags() -> CGEventFlags {
        var flags: CGEventFlags = []
        if pressedKeys.contains(224) || pressedKeys.contains(228) { flags.insert(.maskControl) }
        if pressedKeys.contains(225) || pressedKeys.contains(229) { flags.insert(.maskShift) }
        if pressedKeys.contains(226) || pressedKeys.contains(230) { flags.insert(.maskAlternate) }
        if pressedKeys.contains(227) || pressedKeys.contains(231) { flags.insert(.maskCommand) }
        return flags
    }
    func inject(_ payload: [String: Any]) -> Bool {
        injection.perform(until:Date().addingTimeInterval(0.25)) { [self] in
            state.lock(); let unavailable = shutdownRequested || emergencyPending; state.unlock()
            guard !unavailable else { return false }
            guard let kind = payload["kind"] as? String else { return false }
            let current = CGEvent(source: nil)?.location ?? .zero
            switch kind {
            case "move":
                guard let dx = payload["dx"] as? Double, let dy = payload["dy"] as? Double else { return false }
                let bounds = Self.desktopBounds()
                guard !bounds.isNull, !bounds.isEmpty else { return false }
                let point = CGPoint(x: min(bounds.maxX - 1, max(bounds.minX,current.x + dx)),
                                    y: min(bounds.maxY - 1, max(bounds.minY,current.y + dy)))
                let button = pressedButtons.sorted().first ?? 0
                let type: CGEventType = pressedButtons.isEmpty ? .mouseMoved : dragType(button)
                post(CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: point,
                     mouseButton: CGMouseButton(rawValue: UInt32(button)) ?? .left))
                emit(["kind":"pointer", "x":point.x, "y":point.y, "injected":true, "dx":0, "dy":0])
            case "button":
                guard let button = payload["button"] as? Int, (0...7).contains(button), let down = payload["down"] as? Bool,
                    let cgButton = CGMouseButton(rawValue: UInt32(button)) else { return false }
                if down { pressedButtons.insert(button) } else { pressedButtons.remove(button) }
                let type = mouseType(button,down:down)
                post(CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: current, mouseButton: cgButton))
            case "wheel":
                guard let dx = payload["dx"] as? Double, let dy = payload["dy"] as? Double else { return false }
                let horizontal = dx + wheelRemainderX; let vertical = dy + wheelRemainderY
                let integralX = horizontal.rounded(.towardZero); let integralY = vertical.rounded(.towardZero)
                wheelRemainderX = horizontal - integralX; wheelRemainderY = vertical - integralY
                if integralX != 0 || integralY != 0 {
                    post(CGEvent(scrollWheelEvent2Source: source, units: .line, wheelCount: 2,
                                 wheel1: Int32(integralY), wheel2: Int32(integralX), wheel3: 0))
                }
            case "key":
                guard let number = payload["code"] as? NSNumber, let raw = UInt16(exactly:number.uint64Value), let code = KmHidKeyMap.hidToMac[raw], let down = payload["down"] as? Bool else { return false }
                let repeating = down && pressedKeys.contains(raw)
                if down { pressedKeys.insert(raw) } else { pressedKeys.remove(raw) }
                let event = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down)
                event?.setIntegerValueField(.keyboardEventAutorepeat, value:repeating ? 1 : 0)
                post(event, keyboard: true)
            case "cursor":
                guard let x = payload["x"] as? Double, let y = payload["y"] as? Double else { return false }
                post(CGEvent(mouseEventSource: source, mouseType: .mouseMoved,
                    mouseCursorPosition: CGPoint(x:x,y:y), mouseButton: .left))
            default: return false
            }
            return true
        }
    }
    @discardableResult func releaseAll() -> Bool {
        state.lock(); pendingReleases += 1; state.unlock()
        return injection.perform(until:Date().addingTimeInterval(0.25)) { [self] in
            releaseAllNow()
            state.lock(); pendingReleases -= 1; state.unlock()
            return true
        }
    }
    private func releaseAllNow() {
        for hid in pressedKeys.sorted() {
            guard let code = KmHidKeyMap.hidToMac[hid] else { continue }
            pressedKeys.remove(hid)
            post(CGEvent(keyboardEventSource:source,virtualKey:code,keyDown:false),keyboard:true)
        }
        pressedKeys.removeAll()
        let point = CGEvent(source:nil)?.location ?? .zero
        for button in pressedButtons.sorted() {
            post(CGEvent(mouseEventSource:source,mouseType:mouseType(button,down:false),mouseCursorPosition:point,
                mouseButton:CGMouseButton(rawValue:UInt32(button)) ?? .left))
        }
        pressedButtons.removeAll(); wheelRemainderX = 0; wheelRemainderY = 0
    }
    /// Returns true only when every producer is retired and Rust may free its callback context.
    @discardableResult func stop() -> Bool {
        let deadline = Date().addingTimeInterval(0.25)
        callbacks.close()
        state.lock(); let first = !shutdownRequested; shutdownRequested = true; state.unlock()
        if first {
            for observer in observers {
                NSWorkspace.shared.notificationCenter.removeObserver(observer)
                DistributedNotificationCenter.default().removeObserver(observer)
            }
            observers.removeAll()
            injection.enqueue { [self] in
                releaseAllNow()
                _ = ownership.setOwner(.local)
                state.lock(); stopped = true; let runLoop = loop; state.unlock()
                if let runLoop { CFRunLoopStop(runLoop) }
            }
        }
        let tapRetired = finished.wait(timeout:.now()+max(0,deadline.timeIntervalSinceNow)) == .success
        let callbacksRetired = callbacks.closeAndWait(until:deadline)
        return tapRetired && callbacksRetired
    }
}
