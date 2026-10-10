import Foundation

/// The raw Rust context is usable only inside a counted emission. Closing prevents late
/// observer delivery; a timed-out drain explicitly leaves context ownership with Rust.
final class KmCallbackFence {
    private let condition = NSCondition()
    private var open = true
    private var active = 0
    func emit(_ body: () -> Void) {
        condition.lock()
        guard open else { condition.unlock(); return }
        active += 1; condition.unlock()
        defer { condition.lock(); active -= 1; condition.broadcast(); condition.unlock() }
        body()
    }
    func close() { condition.lock(); open = false; condition.unlock() }
    func closeAndWait(until deadline: Date) -> Bool {
        condition.lock(); defer { condition.unlock() }
        open = false
        while active > 0 {
            if !condition.wait(until:deadline) { return active == 0 }
        }
        return true
    }
}

/// Event handling and owner commit share the same boundary. A local down cannot
/// pass using a stale policy while a remote owner consumes its corresponding up.
final class KmOwnershipGate {
    private let lock = NSRecursiveLock()
    private var policy = KmEventPolicy()
    private var physical = KmPhysicalState()
    func withEvent<T>(_ body: (inout KmEventPolicy, inout KmPhysicalState) -> T) -> T {
        lock.lock(); defer { lock.unlock() }
        return body(&policy,&physical)
    }
    func setOwner(_ owner: KmNativeOwner, snapshotHeld: () -> Bool = { false }, onCommit: () -> Void = {}) -> Bool {
        withEvent { policy, physical in
            if owner == .macToAndroid && (physical.isHeld || snapshotHeld()) { return false }
            policy.owner = owner
            onCommit()
            return true
        }
    }
}

/// Emergencies enqueue without waiting for earlier injection. Synchronous FFI operations
/// have a deadline; queued work owns its captures until completion even after a timeout.
final class KmWorkQueue {
    private let queue = DispatchQueue(label:"leftcar.km.inject")
    func enqueue(_ body: @escaping () -> Void) { queue.async(execute:body) }
    func perform(until deadline: Date, _ body: @escaping () -> Bool) -> Bool {
        let finished = DispatchSemaphore(value:0)
        let result = KmWorkResult()
        enqueue { result.value = body(); finished.signal() }
        guard finished.wait(timeout:.now()+max(0,deadline.timeIntervalSinceNow)) == .success else { return false }
        return result.value
    }
}
private final class KmWorkResult {
    private let lock = NSLock()
    private var stored = false
    var value: Bool {
        get { lock.lock(); defer { lock.unlock() }; return stored }
        set { lock.lock(); stored = newValue; lock.unlock() }
    }
}
