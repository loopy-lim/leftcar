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

/// 현재(활성) 입력 소스의 언어 태그 목록. 메인 스레드 전용 — TIS가 그렇다.
private func currentInputSourceLanguages() -> [String] {
    dispatchPrecondition(condition: .onQueue(.main))
    let current = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
    return inputSourceProperty(current, kTISPropertyInputSourceLanguages) ?? []
}

extension CaptureSession {
    /// 한/영 하드웨어 키(204)의 폴백: kind-7 언어 동기화 없이 그대로 중계된
    /// LANGUAGE_SWITCH를 활성 입력 소스 전환으로 바꾼다. 기본표의 104 매핑
    /// (kVK_JIS_Kana)은 한국어 소스에서 아무 일도 하지 않으므로 그 자리를
    /// 대신한다. kind-7 동기화가 살아 있는 세션에서는 뷰어가 이 키를 소비해
    /// 여기까지 오지 않는다.
    func toggleInputLanguage() {
        DispatchQueue.main.async { [weak self] in
            guard let self, self.sourceAuthorization?.begin() ?? true else { return }
            defer { self.sourceAuthorization?.end() }
            guard self.inputEnabled else { return }
            let tag = primaryLanguageTag(currentInputSourceLanguages())
            let applied = self.inputLanguageSelector(nextInputLanguage(currentTag: tag))
            leftcarInputLogger.info(
                "language toggle from=\(tag ?? "none", privacy: .public) applied=\(applied)"
            )
        }
    }

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
