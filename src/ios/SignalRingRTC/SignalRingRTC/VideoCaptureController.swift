//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import WebRTC
import SignalCoreKit

public class VideoCaptureController {
    static let outputSizeWidth: Int32 = 1280
    static let outputSizeHeight: Int32 = 720
    static let outputFrameRate: Int32 = 30

    private let capturer = RTCCameraVideoCapturer()
    var capturerDelegate: RTCVideoCapturerDelegate? {
        set { capturer.delegate = newValue }
        get { capturer.delegate }
    }
    private let serialQueue = DispatchQueue(label: "org.signal.videoCaptureController")
    private var isUsingFrontCamera: Bool = true
    private var isCapturing: Bool = false

    public var captureSession: AVCaptureSession {
        return capturer.captureSession
    }

    public init() {}

    public func startCapture() {
        serialQueue.sync { [weak self] in
            guard let strongSelf = self else {
                return
            }

            // Don't call startCapture if we're actively capturing.
            guard !strongSelf.isCapturing else { return }

            strongSelf.startCaptureSync()
        }
    }

    public func stopCapture() {
        serialQueue.sync { [weak self] in
            guard let strongSelf = self else {
                return
            }

            // Don't call stopCapture unless we're actively capturing.
            // Calling this when we're not capturing will result in
            // a crash on iOS 13 when built with the iOS 13 SDK.
            guard strongSelf.isCapturing else { return }

            strongSelf.capturer.stopCapture()
            strongSelf.isCapturing = false
        }
    }

    public func switchCamera(isUsingFrontCamera: Bool) {
        serialQueue.sync { [weak self] in
            guard let strongSelf = self else {
                return
            }

            // Only restart capturing again if the camera changes.
            if strongSelf.isUsingFrontCamera != isUsingFrontCamera {
                strongSelf.isUsingFrontCamera = isUsingFrontCamera
                strongSelf.startCaptureSync()
            }
        }
    }

    private func assertIsOnSerialQueue() {
        if _isDebugAssertConfiguration(), #available(iOS 10.0, *) {
            assertOnQueue(serialQueue)
        }
    }

    private func startCaptureSync() {
        assertIsOnSerialQueue()

        let position: AVCaptureDevice.Position = isUsingFrontCamera ? .front : .back
        guard let device: AVCaptureDevice = self.device(position: position) else {
            owsFailDebug("unable to find captureDevice")
            return
        }

        guard let format: AVCaptureDevice.Format = self.format(device: device) else {
            owsFailDebug("unable to find captureDevice")
            return
        }

        let fps = self.framesPerSecond(format: format)
        capturer.startCapture(with: device, format: format, fps: fps)
        isCapturing = true
    }

    private func device(position: AVCaptureDevice.Position) -> AVCaptureDevice? {
        let captureDevices = RTCCameraVideoCapturer.captureDevices()
        guard let device = (captureDevices.first { $0.position == position }) else {
            Logger.debug("unable to find desired position: \(position)")
            return captureDevices.first
        }

        return device
    }

    private func format(device: AVCaptureDevice) -> AVCaptureDevice.Format? {
        let formats = RTCCameraVideoCapturer.supportedFormats(for: device)

        // For rendering, find a format that most closely matches the display size.
        // The local camera capture may be rendered full screen. However, make sure
        // the camera capture is at least our output size, which should be available
        // on all devices the client supports.
        let screenSize = UIScreen.main.nativeBounds.size
        let targetWidth = max(Int32(screenSize.width), Self.outputSizeWidth)
        let targetHeight = max(Int32(screenSize.height), Self.outputSizeHeight)

        var selectedFormat: AVCaptureDevice.Format?
        var currentDiff: Int32 = Int32.max

        for format in formats {
            let dimension = CMVideoFormatDescriptionGetDimensions(format.formatDescription)
            let diff = abs(targetWidth - dimension.width) + abs(targetHeight - dimension.height)
            if diff < currentDiff {
                selectedFormat = format
                currentDiff = diff
            }
        }

        if _isDebugAssertConfiguration(), let selectedFormat = selectedFormat {
            let dimension = CMVideoFormatDescriptionGetDimensions(selectedFormat.formatDescription)
            Logger.debug("selected format width: \(dimension.width) height: \(dimension.height)")
        }

        assert(selectedFormat != nil)

        return selectedFormat
    }

    private func framesPerSecond(format: AVCaptureDevice.Format) -> Int {
        var maxFrameRate: Float64 = 0
        for range in format.videoSupportedFrameRateRanges {
            maxFrameRate = max(maxFrameRate, range.maxFrameRate)
        }

        return Int(maxFrameRate)
    }
}
