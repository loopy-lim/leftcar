import Foundation
import CoreGraphics

@main struct KmEventPolicyTests {
    static func main() {
        var policy = KmEventPolicy()
        let escape: CGEventFlags = [.maskControl, .maskAlternate, .maskCommand]
        precondition(policy.isEscape(key: 53, flags: escape), "native emergency chord must be recognized")
        precondition(!policy.isEscape(key: 53, flags: [.maskControl]), "partial chord must remain ordinary input")
        policy.owner = .macToAndroid
        precondition(policy.suppress(tag: 0), "physical input must follow remote pointer")
        precondition(!policy.suppress(tag: kmSyntheticTag), "injection must never be recaptured")
        policy.owner = .androidToMac
        precondition(!policy.suppress(tag: 0), "Android origin must not suppress Mac physical mouse")
        policy.owner = .local
        precondition(!policy.suppress(tag: 0), "armed observation must preserve local input")
        let union = CGRect(x: -1920, y: -200, width: 4480, height: 1640)
        precondition(policy.nearSharedEdge(point: CGPoint(x: -1919, y: 0), bounds: union, left: true))
        precondition(!policy.nearSharedEdge(point: CGPoint(x: 100, y: 0), bounds: union, left: true))
        var held = KmPhysicalState()
        held.key(56, down: true)
        precondition(held.isHeld, "held Shift must defer edge transfer")
        held.key(56, down: false)
        precondition(!held.isHeld, "local Shift up must allow later transfer")
        held.button(0, down: true)
        precondition(held.isHeld, "local drag cannot be stolen at edge")
        held.button(0, down: false)
        precondition(!held.isHeld)
        precondition(KmHidKeyMap.macToHid[0] == 4, "Mac A must carry HID usage 4")
        precondition(KmHidKeyMap.hidToMac[224] == 59, "left Ctrl must inject physical Mac Ctrl")
        let fence = KmCallbackFence()
        let entered = DispatchSemaphore(value:0)
        let resume = DispatchSemaphore(value:0)
        let finished = DispatchSemaphore(value:0)
        DispatchQueue.global().async {
            fence.emit { entered.signal(); resume.wait() }
            finished.signal()
        }
        precondition(entered.wait(timeout:.now()+1) == .success)
        precondition(!fence.closeAndWait(until:Date().addingTimeInterval(0.02)), "paused emission must retain callback context")
        var lateEmission = false
        fence.emit { lateEmission = true }
        precondition(!lateEmission, "late observer cannot touch retired Rust context")
        resume.signal()
        precondition(finished.wait(timeout:.now()+1) == .success)
        precondition(fence.closeAndWait(until:Date().addingTimeInterval(1)))

        let observerFence = KmCallbackFence()
        let observerPaused = DispatchSemaphore(value:0)
        let observerResume = DispatchSemaphore(value:0)
        let observerFinished = DispatchSemaphore(value:0)
        DispatchQueue.global().async {
            // Deterministic notification paused after cleanup, immediately before emission.
            observerFence.emit {
                observerPaused.signal(); observerResume.wait()
                observerFence.emit { preconditionFailure("retired observer used Rust context") }
            }
            observerFinished.signal()
        }
        precondition(observerPaused.wait(timeout:.now()+1) == .success)
        precondition(!observerFence.closeAndWait(until:Date().addingTimeInterval(0.02)), "in-flight observer producer must retain context")
        observerResume.signal()
        precondition(observerFinished.wait(timeout:.now()+1) == .success)
        precondition(observerFence.closeAndWait(until:Date().addingTimeInterval(1)))

        let control = KmWorkQueue()
        let injectionBusy = DispatchSemaphore(value:0)
        let injectionResume = DispatchSemaphore(value:0)
        let releaseFinished = DispatchSemaphore(value:0)
        control.enqueue { injectionBusy.signal(); injectionResume.wait() }
        precondition(injectionBusy.wait(timeout:.now()+1) == .success)
        let emergencyStarted = Date()
        control.enqueue { releaseFinished.signal() }
        precondition(Date().timeIntervalSince(emergencyStarted) < 0.05, "tap emergency must only enqueue behind busy injection")
        precondition(!control.perform(until:Date().addingTimeInterval(0.02), { true }), "FFI shutdown/injection wait must be bounded")
        precondition(releaseFinished.wait(timeout:.now()+0.02) == .timedOut, "local restoration must wait for actual release")
        injectionResume.signal()
        precondition(releaseFinished.wait(timeout:.now()+1) == .success)
        precondition(control.perform(until:Date().addingTimeInterval(1), { true }))

        let ownership = KmOwnershipGate()
        let downStarted = DispatchSemaphore(value:0)
        let downResume = DispatchSemaphore(value:0)
        let downFinished = DispatchSemaphore(value:0)
        let commitFinished = DispatchSemaphore(value:0)
        DispatchQueue.global().async {
            ownership.withEvent { _, physical in
                physical.key(0,down:true)
                downStarted.signal(); downResume.wait()
            }
            downFinished.signal()
        }
        precondition(downStarted.wait(timeout:.now()+1) == .success)
        DispatchQueue.global().async {
            precondition(!ownership.setOwner(.macToAndroid,snapshotHeld:{false}), "interleaved local down must reject suppression commit")
            commitFinished.signal()
        }
        precondition(commitFinished.wait(timeout:.now()+0.02) == .timedOut, "commit cannot overtake an in-flight local event")
        downResume.signal()
        precondition(downFinished.wait(timeout:.now()+1) == .success)
        precondition(commitFinished.wait(timeout:.now()+1) == .success)
        ownership.withEvent { policy, physical in
            precondition(policy.owner == .local && !policy.suppress(tag:0))
            physical.key(0,down:false)
        }
        var modifierCommitCount = 0
        precondition(ownership.setOwner(.macToAndroid,snapshotHeld:{false},onCommit:{modifierCommitCount += 1}))
        precondition(modifierCommitCount == 1, "modifier state resets only at committed neutral boundary")
        print("KmEventPolicyTests PASS: policy, HID, callback fence, paused observer, held-state interleaving, bounded emergency queue")
    }
}
