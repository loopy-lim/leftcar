import Foundation

 func appendRollingSample(_ value: UInt64, to samples: inout [UInt64]) {
    samples.append(value)
    if samples.count > 300 {
        samples.removeFirst(samples.count - 300)
    }
}

 func percentile(_ samples: [UInt64], quantile: Double) -> UInt64 {
    guard !samples.isEmpty else { return 0 }
    let sorted = samples.sorted()
    let boundedQuantile = min(1.0, max(0.0, quantile))
    let index = min(sorted.count - 1, Int(ceil(Double(sorted.count) * boundedQuantile)) - 1)
    return sorted[max(0, index)]
}

 func percentile95(_ samples: [UInt64]) -> UInt64 {
    percentile(samples, quantile: 0.95)
}

final class PerformanceLogTicker {
     let interval: DispatchTimeInterval
     let queue: DispatchQueue
     let queueKey = DispatchSpecificKey<Void>()
     let handler: () -> Void
     var timer: DispatchSourceTimer?

    init(
        interval: DispatchTimeInterval,
        queue: DispatchQueue,
        handler: @escaping () -> Void
    ) {
        self.interval = interval
        self.queue = queue
        self.handler = handler
        queue.setSpecific(key: queueKey, value: ())
    }

    @discardableResult
    func start() -> Bool {
        synchronized {
            guard timer == nil else { return false }
            let source = DispatchSource.makeTimerSource(queue: queue)
            source.schedule(
                deadline: .now() + interval,
                repeating: interval,
                leeway: .milliseconds(1)
            )
            source.setEventHandler(handler: handler)
            timer = source
            source.activate()
            return true
        }
    }

    func stop() {
        synchronized {
            guard let source = timer else { return }
            timer = nil
            source.setEventHandler {}
            source.cancel()
        }
    }

    deinit {
        stop()
    }

     func synchronized<T>(_ body: () -> T) -> T {
        if DispatchQueue.getSpecific(key: queueKey) != nil {
            return body()
        }
        return queue.sync(execute: body)
    }
}
