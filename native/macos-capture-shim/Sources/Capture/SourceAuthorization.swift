import Foundation

/// Host-owned opaque lease. Native retains the context until every session
/// callback retires. begin/end bracket actual setup/output, never registry locks.
final class SourceAuthorization {
    let context: UnsafeMutableRawPointer
    let beginOperation: @convention(c) (UnsafeMutableRawPointer?) -> Int32
    let endOperation: @convention(c) (UnsafeMutableRawPointer?) -> Void
    let releaseContext: @convention(c) (UnsafeMutableRawPointer?) -> Void
    init(context: UnsafeMutableRawPointer,
         begin: @escaping @convention(c) (UnsafeMutableRawPointer?) -> Int32,
         end: @escaping @convention(c) (UnsafeMutableRawPointer?) -> Void,
         release: @escaping @convention(c) (UnsafeMutableRawPointer?) -> Void) {
        self.context = context; beginOperation = begin; endOperation = end; releaseContext = release
    }
    func begin() -> Bool { beginOperation(context) != 0 }
    func end() { endOperation(context) }
    deinit { releaseContext(context) }
}
