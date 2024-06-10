//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

@testable import SignalRingRTC
import WebRTC

public class AudioDeviceModuleForTests: NSObject, RTCAudioDevice {
    public var deviceInputSampleRate: Double = 48000.0
    public var inputIOBufferDuration: TimeInterval = 0.020
    public var inputNumberOfChannels: Int = 1
    public var inputLatency: TimeInterval = 0.0
    public var deviceOutputSampleRate: Double = 48000.0
    public var outputIOBufferDuration: TimeInterval = 0.020
    public var outputNumberOfChannels: Int = 1
    public var outputLatency: TimeInterval = 0.0
    public var isInitialized: Bool

    override init() {
        Logger.debug("Dummy ADM for testing.")
        isInitialized = false
        isPlayoutInitialized = false
        isPlaying = false
        isRecordingInitialized = false
        isRecording = false
    }

    public func initialize(with delegate: any RTCAudioDeviceDelegate) -> Bool {
        isInitialized = true
        return true
    }

    public func terminateDevice() -> Bool {
        isInitialized = false
        isPlayoutInitialized = false
        isPlaying = false
        isRecordingInitialized = false
        isRecording = false
        return true
    }

    public var isPlayoutInitialized: Bool

    public func initializePlayout() -> Bool {
        isPlayoutInitialized = true
        return true
    }

    public var isPlaying: Bool

    public func startPlayout() -> Bool {
        isPlaying = true
        return true
    }

    public func stopPlayout() -> Bool {
        isPlaying = false
        return true
    }

    public var isRecordingInitialized: Bool

    public func initializeRecording() -> Bool {
        isRecordingInitialized = true
        return true
    }

    public var isRecording: Bool

    public func startRecording() -> Bool {
        isRecording = true
        return true
    }

    public func stopRecording() -> Bool {
        isRecording = false
        return true
    }
}
