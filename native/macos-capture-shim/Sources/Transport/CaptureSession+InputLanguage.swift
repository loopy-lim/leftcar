import Foundation
import Carbon

private func inputSourceProperty<T>(_ source: TISInputSource, _ key: CFString) -> T? {
    guard let pointer = TISGetInputSourceProperty(source, key) else { return nil }
    return Unmanaged<AnyObject>.fromOpaque(pointer).takeUnretainedValue() as? T
}

/// Text Input Source Services is main-thread-only in a UI process. This selects
/// an already-enabled native input method; it never changes shortcuts or layouts.
func selectNativeInputLanguage(_ language: UInt8) -> Bool {
    dispatchPrecondition(condition: .onQueue(.main))
    let list = TISCreateInputSourceList(nil, false).takeRetainedValue() as! [TISInputSource]
    let current = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
    let currentID: String? = inputSourceProperty(current, kTISPropertyInputSourceID)
    let candidates = list.map { source in
        InputLanguageSource(
            id: inputSourceProperty(source, kTISPropertyInputSourceID) ?? "",
            languages: inputSourceProperty(source, kTISPropertyInputSourceLanguages) ?? [],
            selectable: inputSourceProperty(source, kTISPropertyInputSourceIsSelectCapable) ?? false
        )
    }
    guard let target = inputLanguageSource(language, current: currentID, sources: candidates) else { return false }
    if target == currentID { return true }
    guard let index = candidates.firstIndex(where: { $0.id == target }) else { return false }
    return TISSelectInputSource(list[index]) == noErr
}

extension CaptureSession {
    func handleInputLanguage(_ message: Data, sequence: UInt32, fd: Int32, destination: sockaddr_in?) {
        guard message.count == 11, message[10] == 1 || message[10] == 2 else { return }
        inputLock.lock()
        guard lastReliableInputSequence &+ 1 == sequence else {
            let duplicate = lastReliableInputSequence == sequence
            inputLock.unlock()
            if duplicate { sendInputAck(sequence: sequence, fd: fd, destination: destination) }
            return
        }
        guard inputLanguageTransition.begin(sequence: sequence) else { inputLock.unlock(); return }
        let generation = inputLanguageTransition.generation
        inputLock.unlock()
        let language = message[10]
        DispatchQueue.main.async { [weak self] in
            guard let self, self.sourceAuthorization?.begin() ?? true else { return }
            defer { self.sourceAuthorization?.end() }
            self.inputLock.lock()
            guard self.inputLanguageTransition.generation == generation,
                  self.inputLanguageTransition.sequence == sequence,
                  self.lastReliableInputSequence &+ 1 == sequence else {
                self.inputLock.unlock()
                return
            }
            if self.inputEnabled {
                let applied = self.inputLanguageSelector(language)
                leftcarInputLogger.info("input language requested=\(language) applied=\(applied)")
            }
            _ = self.inputLanguageTransition.complete(sequence: sequence)
            self.lastReliableInputSequence = sequence
            self.inputLock.unlock()
            // No ACK until the source switch has completed, so the sender's
            // reliable queue cannot deliver the following letter too early.
            self.inputQueue.async { [weak self] in
                guard let self, self.sock == fd, fd >= 0 else { return }
                self.sendInputAck(sequence: sequence, fd: fd, destination: destination)
            }
        }
    }
}
