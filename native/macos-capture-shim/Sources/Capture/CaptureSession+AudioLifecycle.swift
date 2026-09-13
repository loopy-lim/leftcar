import Foundation

extension CaptureSession {
    /// Complete any in-flight conversion/send before closing its socket or
    /// transferring ownership. Reentrant queue calls never synchronously wait
    /// on themselves; every other caller observes actual queue completion.
    func retireAudioEncoder() {
        if DispatchQueue.getSpecific(key: audioQueueKey) != nil {
            opusAudioEncoder = nil
        } else {
            audioQueue.sync { opusAudioEncoder = nil }
        }
    }
}
