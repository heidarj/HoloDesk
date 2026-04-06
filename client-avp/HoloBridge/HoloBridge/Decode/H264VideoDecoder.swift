import CoreMedia
import CoreVideo
import Foundation
import HoloBridgeClientCore
import VideoToolbox

public enum H264VideoDecoderError: Error, LocalizedError {
    case missingNALUnits
    case missingParameterSets
    case formatDescription(OSStatus)
    case decompressionSession(OSStatus)
    case blockBuffer(OSStatus)
    case sampleBuffer(OSStatus)
    case decode(OSStatus)

    public var errorDescription: String? {
        switch self {
        case .missingNALUnits:
            return "The Annex-B access unit did not contain any NAL units"
        case .missingParameterSets:
            return "The keyframe did not include SPS/PPS parameter sets"
        case .formatDescription(let status):
            return "Failed to create CMVideoFormatDescription (\(status))"
        case .decompressionSession(let status):
            return "Failed to create VTDecompressionSession (\(status))"
        case .blockBuffer(let status):
            return "Failed to create CMBlockBuffer (\(status))"
        case .sampleBuffer(let status):
            return "Failed to create CMSampleBuffer (\(status))"
        case .decode(let status):
            return "VideoToolbox decode failed (\(status))"
        }
    }
}

public final class H264VideoDecoder {
    public typealias FrameHandler = @Sendable (CVPixelBuffer) -> Void
    public typealias FormatHandler = @Sendable (CMVideoDimensions) -> Void
    public typealias IssueHandler = @Sendable (String) -> Void

    private let onFrameDecoded: FrameHandler
    private let onFormatDescriptionUpdated: FormatHandler
    private let onIssue: IssueHandler

    private var formatDescription: CMVideoFormatDescription?
    private var decompressionSession: VTDecompressionSession?
    private var cachedSPS: Data?
    private var cachedPPS: Data?
    private var needsKeyframe = true

    public init(
        onFrameDecoded: @escaping FrameHandler,
        onFormatDescriptionUpdated: @escaping FormatHandler = { _ in },
        onIssue: @escaping IssueHandler = { _ in }
    ) {
        self.onFrameDecoded = onFrameDecoded
        self.onFormatDescriptionUpdated = onFormatDescriptionUpdated
        self.onIssue = onIssue
    }

    deinit {
        reset()
    }

    public func reset() {
        if let decompressionSession {
            VTDecompressionSessionWaitForAsynchronousFrames(decompressionSession)
            VTDecompressionSessionInvalidate(decompressionSession)
        }
        decompressionSession = nil
        formatDescription = nil
        cachedSPS = nil
        cachedPPS = nil
        needsKeyframe = true
    }

    public func decode(accessUnit: H264VideoAccessUnit) throws {
        let nalUnits = try Self.parseAnnexBNALUnits(from: accessUnit.data)

        if accessUnit.isKeyframe {
            try updateDecoderConfiguration(from: nalUnits)
            needsKeyframe = false
        }

        guard !needsKeyframe else {
            return
        }

        guard
            let formatDescription,
            let decompressionSession
        else {
            needsKeyframe = true
            return
        }

        let avccData = Self.makeLengthPrefixedSample(from: nalUnits)
        let sampleBuffer = try Self.makeSampleBuffer(
            sampleData: avccData,
            formatDescription: formatDescription,
            pts100ns: accessUnit.pts100ns,
            duration100ns: accessUnit.duration100ns
        )

        var infoFlags = VTDecodeInfoFlags()
        let status = VTDecompressionSessionDecodeFrame(
            decompressionSession,
            sampleBuffer: sampleBuffer,
            flags: [],
            frameRefcon: nil,
            infoFlagsOut: &infoFlags
        )

        guard status == noErr else {
            needsKeyframe = true
            if accessUnit.isKeyframe {
                throw H264VideoDecoderError.decode(status)
            }
            onIssue("Dropped delta frame after decode error (\(status)); waiting for the next keyframe.")
            return
        }
    }

    private func updateDecoderConfiguration(from nalUnits: [Data]) throws {
        var sps: Data?
        var pps: Data?

        for nalUnit in nalUnits {
            guard let firstByte = nalUnit.first else {
                continue
            }
            switch firstByte & 0x1F {
            case 7:
                sps = nalUnit
            case 8:
                pps = nalUnit
            default:
                break
            }
        }

        guard let sps, let pps else {
            throw H264VideoDecoderError.missingParameterSets
        }

        guard cachedSPS != sps || cachedPPS != pps || formatDescription == nil || decompressionSession == nil else {
            return
        }

        cachedSPS = sps
        cachedPPS = pps
        formatDescription = try Self.makeFormatDescription(sps: sps, pps: pps)
        try recreateDecompressionSession()
    }

    private func recreateDecompressionSession() throws {
        if let decompressionSession {
            VTDecompressionSessionWaitForAsynchronousFrames(decompressionSession)
            VTDecompressionSessionInvalidate(decompressionSession)
            self.decompressionSession = nil
        }

        guard let formatDescription else {
            return
        }

        let imageBufferAttributes: [String: Any] = [
            kCVPixelBufferPixelFormatTypeKey as String: Int(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange),
            kCVPixelBufferMetalCompatibilityKey as String: true,
        ]

        var callbackRecord = VTDecompressionOutputCallbackRecord(
            decompressionOutputCallback: Self.decompressionOutputCallback,
            decompressionOutputRefCon: UnsafeMutableRawPointer(Unmanaged.passUnretained(self).toOpaque())
        )

        var newSession: VTDecompressionSession?
        let status = VTDecompressionSessionCreate(
            allocator: kCFAllocatorDefault,
            formatDescription: formatDescription,
            decoderSpecification: nil,
            imageBufferAttributes: imageBufferAttributes as CFDictionary,
            outputCallback: &callbackRecord,
            decompressionSessionOut: &newSession
        )
        guard status == noErr, let newSession else {
            throw H264VideoDecoderError.decompressionSession(status)
        }

        decompressionSession = newSession
        onFormatDescriptionUpdated(CMVideoFormatDescriptionGetDimensions(formatDescription))
    }

    private func handleDecodeCallback(
        status: OSStatus,
        imageBuffer: CVImageBuffer?
    ) {
        guard status == noErr, let imageBuffer else {
            needsKeyframe = true
            onIssue("Decoder lost sync after VideoToolbox callback error (\(status)); waiting for the next keyframe.")
            return
        }

        needsKeyframe = false
        onFrameDecoded(imageBuffer)
    }

    private static let decompressionOutputCallback: VTDecompressionOutputCallback = {
        decompressionOutputRefCon,
        _,
        status,
        _,
        imageBuffer,
        _,
        _
        in
        guard let decompressionOutputRefCon else {
            return
        }
        let decoder = Unmanaged<H264VideoDecoder>.fromOpaque(decompressionOutputRefCon).takeUnretainedValue()
        decoder.handleDecodeCallback(status: status, imageBuffer: imageBuffer)
    }

    private static func makeFormatDescription(
        sps: Data,
        pps: Data
    ) throws -> CMVideoFormatDescription {
        var formatDescription: CMFormatDescription?

        let status = sps.withUnsafeBytes { spsBytes in
            pps.withUnsafeBytes { ppsBytes in
                let parameterSetPointers = [
                    spsBytes.bindMemory(to: UInt8.self).baseAddress!,
                    ppsBytes.bindMemory(to: UInt8.self).baseAddress!,
                ]
                let parameterSetSizes = [sps.count, pps.count]

                return CMVideoFormatDescriptionCreateFromH264ParameterSets(
                    allocator: kCFAllocatorDefault,
                    parameterSetCount: parameterSetPointers.count,
                    parameterSetPointers: parameterSetPointers,
                    parameterSetSizes: parameterSetSizes,
                    nalUnitHeaderLength: 4,
                    formatDescriptionOut: &formatDescription
                )
            }
        }

        guard status == noErr, let formatDescription else {
            throw H264VideoDecoderError.formatDescription(status)
        }

        return formatDescription
    }

    private static func makeSampleBuffer(
        sampleData: Data,
        formatDescription: CMVideoFormatDescription,
        pts100ns: Int64,
        duration100ns: Int64
    ) throws -> CMSampleBuffer {
        var blockBuffer: CMBlockBuffer?
        let blockBufferStatus = CMBlockBufferCreateWithMemoryBlock(
            allocator: kCFAllocatorDefault,
            memoryBlock: nil,
            blockLength: sampleData.count,
            blockAllocator: nil,
            customBlockSource: nil,
            offsetToData: 0,
            dataLength: sampleData.count,
            flags: 0,
            blockBufferOut: &blockBuffer
        )
        guard blockBufferStatus == kCMBlockBufferNoErr, let blockBuffer else {
            throw H264VideoDecoderError.blockBuffer(blockBufferStatus)
        }

        let replaceStatus = sampleData.withUnsafeBytes { bytes in
            CMBlockBufferReplaceDataBytes(
                with: bytes.baseAddress!,
                blockBuffer: blockBuffer,
                offsetIntoDestination: 0,
                dataLength: sampleData.count
            )
        }
        guard replaceStatus == kCMBlockBufferNoErr else {
            throw H264VideoDecoderError.blockBuffer(replaceStatus)
        }

        var timingInfo = CMSampleTimingInfo(
            duration: CMTime(value: duration100ns, timescale: 10_000_000),
            presentationTimeStamp: CMTime(value: pts100ns, timescale: 10_000_000),
            decodeTimeStamp: .invalid
        )
        let sampleSizes = [sampleData.count]
        var sampleBuffer: CMSampleBuffer?
        let sampleStatus = CMSampleBufferCreateReady(
            allocator: kCFAllocatorDefault,
            dataBuffer: blockBuffer,
            formatDescription: formatDescription,
            sampleCount: 1,
            sampleTimingEntryCount: 1,
            sampleTimingArray: &timingInfo,
            sampleSizeEntryCount: 1,
            sampleSizeArray: sampleSizes,
            sampleBufferOut: &sampleBuffer
        )
        guard sampleStatus == noErr, let sampleBuffer else {
            throw H264VideoDecoderError.sampleBuffer(sampleStatus)
        }

        return sampleBuffer
    }

    private static func makeLengthPrefixedSample(from nalUnits: [Data]) -> Data {
        nalUnits.reduce(into: Data()) { sampleData, nalUnit in
            var nalLength = UInt32(nalUnit.count).bigEndian
            withUnsafeBytes(of: &nalLength) { sampleData.append(contentsOf: $0) }
            sampleData.append(nalUnit)
        }
    }

    private static func parseAnnexBNALUnits(from data: Data) throws -> [Data] {
        var nalUnits: [Data] = []
        var index = 0

        while index < data.count {
            let startCodeLength = annexBStartCodeLength(in: data, at: index)
            guard startCodeLength > 0 else {
                index += 1
                continue
            }

            let nalStart = index + startCodeLength
            var nalEnd = nalStart
            while nalEnd < data.count && annexBStartCodeLength(in: data, at: nalEnd) == 0 {
                nalEnd += 1
            }

            if nalStart < nalEnd {
                nalUnits.append(data.subdata(in: nalStart..<nalEnd))
            }
            index = nalEnd
        }

        guard !nalUnits.isEmpty else {
            throw H264VideoDecoderError.missingNALUnits
        }
        return nalUnits
    }

    private static func annexBStartCodeLength(
        in data: Data,
        at offset: Int
    ) -> Int {
        guard offset + 3 <= data.count else {
            return 0
        }
        guard data[offset] == 0, data[offset + 1] == 0 else {
            return 0
        }
        if data[offset + 2] == 1 {
            return 3
        }
        guard offset + 4 <= data.count else {
            return 0
        }
        return data[offset + 2] == 0 && data[offset + 3] == 1 ? 4 : 0
    }
}
