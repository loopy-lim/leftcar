import Foundation
import CoreVideo
import Metal

final class MetalNv12Splitter {
    struct PreparedPair {
        let left: CVPixelBuffer
        let right: CVPixelBuffer
        let preparationUs: UInt64
    }

    enum SplitError: Error {
        case unsupportedFormat
        case allocation(OSStatus)
        case texture
        case command
    }

    private let geometry: SplitGeometry
    private let commandQueue: MTLCommandQueue
    private let textureCache: CVMetalTextureCache
    private let leftPool: CVPixelBufferPool
    private let rightPool: CVPixelBufferPool

    init(
        geometry: SplitGeometry = .vertical4K,
        pairedInFlightLimit: Int = 3
    ) throws {
        guard let device = MTLCreateSystemDefaultDevice(),
              let commandQueue = device.makeCommandQueue() else {
            throw SplitError.command
        }
        var cache: CVMetalTextureCache?
        let cacheStatus = CVMetalTextureCacheCreate(
            kCFAllocatorDefault,
            nil,
            device,
            nil,
            &cache
        )
        guard cacheStatus == kCVReturnSuccess, let cache else {
            throw SplitError.texture
        }
        self.geometry = geometry
        self.commandQueue = commandQueue
        self.textureCache = cache
        self.leftPool = try Self.makePool(
            region: geometry.left,
            minimumBufferCount: pairedInFlightLimit + 1
        )
        self.rightPool = try Self.makePool(
            region: geometry.right,
            minimumBufferCount: pairedInFlightLimit + 1
        )
    }

    func prepare(
        source: CVPixelBuffer,
        completion: @escaping (Result<PreparedPair, SplitError>) -> Void
    ) {
        let startedNs = DispatchTime.now().uptimeNanoseconds
        guard CVPixelBufferGetPixelFormatType(source)
                == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
              CVPixelBufferGetPlaneCount(source) == 2,
              CVPixelBufferGetWidth(source) == geometry.fullWidth,
              CVPixelBufferGetHeight(source) == geometry.fullHeight else {
            completion(.failure(.unsupportedFormat))
            return
        }

        var left: CVPixelBuffer?
        var right: CVPixelBuffer?
        let leftStatus = CVPixelBufferPoolCreatePixelBuffer(
            kCFAllocatorDefault,
            leftPool,
            &left
        )
        let rightStatus = CVPixelBufferPoolCreatePixelBuffer(
            kCFAllocatorDefault,
            rightPool,
            &right
        )
        guard leftStatus == kCVReturnSuccess, let left else {
            completion(.failure(.allocation(leftStatus)))
            return
        }
        guard rightStatus == kCVReturnSuccess, let right else {
            completion(.failure(.allocation(rightStatus)))
            return
        }

        guard let sourceY = texture(
            pixelBuffer: source,
            plane: 0,
            format: .r8Unorm,
            width: geometry.fullWidth,
            height: geometry.fullHeight
        ),
        let sourceUV = texture(
            pixelBuffer: source,
            plane: 1,
            format: .rg8Unorm,
            width: geometry.fullWidth / 2,
            height: geometry.fullHeight / 2
        ),
        let leftY = texture(
            pixelBuffer: left,
            plane: 0,
            format: .r8Unorm,
            width: geometry.left.width,
            height: geometry.left.height
        ),
        let leftUV = texture(
            pixelBuffer: left,
            plane: 1,
            format: .rg8Unorm,
            width: geometry.left.chromaWidth,
            height: geometry.left.chromaHeight
        ),
        let rightY = texture(
            pixelBuffer: right,
            plane: 0,
            format: .r8Unorm,
            width: geometry.right.width,
            height: geometry.right.height
        ),
        let rightUV = texture(
            pixelBuffer: right,
            plane: 1,
            format: .rg8Unorm,
            width: geometry.right.chromaWidth,
            height: geometry.right.chromaHeight
        ),
        let commandBuffer = commandQueue.makeCommandBuffer(),
        let blit = commandBuffer.makeBlitCommandEncoder() else {
            completion(.failure(.texture))
            return
        }

        copy(
            sourceY,
            sourceX: geometry.left.lumaX,
            width: geometry.left.width,
            height: geometry.left.height,
            to: leftY,
            using: blit
        )
        copy(
            sourceUV,
            sourceX: geometry.left.chromaX,
            width: geometry.left.chromaWidth,
            height: geometry.left.chromaHeight,
            to: leftUV,
            using: blit
        )
        copy(
            sourceY,
            sourceX: geometry.right.lumaX,
            width: geometry.right.width,
            height: geometry.right.height,
            to: rightY,
            using: blit
        )
        copy(
            sourceUV,
            sourceX: geometry.right.chromaX,
            width: geometry.right.chromaWidth,
            height: geometry.right.chromaHeight,
            to: rightUV,
            using: blit
        )
        blit.endEncoding()
        commandBuffer.addCompletedHandler { commandBuffer in
            guard commandBuffer.status == .completed else {
                completion(.failure(.command))
                return
            }
            let elapsedUs = (
                DispatchTime.now().uptimeNanoseconds &- startedNs
            ) / 1_000
            completion(
                .success(
                    PreparedPair(
                        left: left,
                        right: right,
                        preparationUs: elapsedUs
                    )
                )
            )
        }
        commandBuffer.commit()
    }

    private static func makePool(
        region: TilePlaneRegion,
        minimumBufferCount: Int
    ) throws -> CVPixelBufferPool {
        let poolAttributes = [
            kCVPixelBufferPoolMinimumBufferCountKey as String: minimumBufferCount,
        ] as CFDictionary
        let pixelAttributes = [
            kCVPixelBufferPixelFormatTypeKey as String:
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferWidthKey as String: region.width,
            kCVPixelBufferHeightKey as String: region.height,
            kCVPixelBufferMetalCompatibilityKey as String: true,
            kCVPixelBufferIOSurfacePropertiesKey as String: [:] as [String: Any],
        ] as CFDictionary
        var pool: CVPixelBufferPool?
        let status = CVPixelBufferPoolCreate(
            kCFAllocatorDefault,
            poolAttributes,
            pixelAttributes,
            &pool
        )
        guard status == kCVReturnSuccess, let pool else {
            throw SplitError.allocation(status)
        }
        return pool
    }

    private func texture(
        pixelBuffer: CVPixelBuffer,
        plane: Int,
        format: MTLPixelFormat,
        width: Int,
        height: Int
    ) -> MTLTexture? {
        var wrapped: CVMetalTexture?
        let status = CVMetalTextureCacheCreateTextureFromImage(
            kCFAllocatorDefault,
            textureCache,
            pixelBuffer,
            nil,
            format,
            width,
            height,
            plane,
            &wrapped
        )
        guard status == kCVReturnSuccess, let wrapped else { return nil }
        return CVMetalTextureGetTexture(wrapped)
    }

    private func copy(
        _ source: MTLTexture,
        sourceX: Int,
        width: Int,
        height: Int,
        to destination: MTLTexture,
        using blit: MTLBlitCommandEncoder
    ) {
        blit.copy(
            from: source,
            sourceSlice: 0,
            sourceLevel: 0,
            sourceOrigin: MTLOrigin(x: sourceX, y: 0, z: 0),
            sourceSize: MTLSize(width: width, height: height, depth: 1),
            to: destination,
            destinationSlice: 0,
            destinationLevel: 0,
            destinationOrigin: MTLOrigin(x: 0, y: 0, z: 0)
        )
    }
}
