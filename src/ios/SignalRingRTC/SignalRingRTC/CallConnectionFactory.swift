//
//  Copyright (c) 2019 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

@objc public class CallConnectionFactory: RTCPeerConnectionFactory {

    // Keep track of the factory object managed by RingRTC.
    private var ringrtcFactory: UnsafeMutableRawPointer?

    // MARK: Object Lifetime

    override public init() {
        // Initialize the global object (mainly for logging).
        _ = CallConnectionGlobal.shared

        let decoderFactory = RTCDefaultVideoDecoderFactory()
        let encoderFactory = RTCDefaultVideoEncoderFactory()

        super.init(encoderFactory: encoderFactory, decoderFactory: decoderFactory)

        let ringrtcFactory = ringRtcCreateCallConnectionFactory(Unmanaged.passUnretained(self).toOpaque())
        if ringrtcFactory == nil {
            owsFailDebug("ringRtcCreateCallConnectionFactory failure")
        }

        self.ringrtcFactory = ringrtcFactory

        Logger.debug("object! CallConnectionFactory created... \(ObjectIdentifier(self))")

        // @temp Print out pointers for verification.
        Logger.debug("ringrtc factory: \(self.ringrtcFactory!)")
    }

    deinit {
        Logger.debug("object! CallConnectionFactory destroyed. \(ObjectIdentifier(self))")
    }

    // MARK: API Functions

    // @note This function blocks and should be called off the
    // main thread.
    public func close() {
        Logger.debug("")

        let retPtr = ringRtcFreeFactory(ringrtcFactory)
        if retPtr == nil {
            Logger.debug("CallConnectionFactory.close failure!")
        }

        Logger.debug("done")
    }

    public func createCallConnection(delegate: CallConnectionDelegate, iceServers: [RTCIceServer], callId: UInt64, isOutgoing: Bool, hideIp: Bool) throws -> CallConnection {
        Logger.debug("CallConnectionFactory.createCallConnection() \(ObjectIdentifier(self))")

        // Create the Call Connection object, no RTC initialization.
        let callConnection = CallConnection(delegate: delegate, factory: self, callId: callId, isOutgoing: isOutgoing)
        Logger.debug("callConnection: \(ObjectIdentifier(callConnection))")

        // We will create an application observer, with callConnection acting as the delegate.
        let observer = CallConnectionObserver(delegate: callConnection)

        // Create a RingRTC observer, passing down ownership of the application
        // observer.
        guard let ringRtcObserver = ringRtcCreateCallConnectionObserver(observer.getWrapper(), callId) else {
            // @todo Confirm cleanup of observer and callConnection. It is automatic?

            throw CallConnectionError.ringRtcCreateFailure(description: "ringRtcCreateCallConnectionObserver() returned failure")
        }

        // We create default configuration settings here as per
        // Signal Messenger policies.

        // Create the configuration.
        let configuration = RTCConfiguration()

        // Update the configuration with the provided Ice Servers.
        // @todo Validate and if none, set a backup value, don't expect
        // application to know what the backup should be.
        configuration.iceServers = iceServers

        // Initialize the configuration.
        configuration.bundlePolicy = .maxBundle
        configuration.rtcpMuxPolicy = .require

        if hideIp {
            configuration.iceTransportPolicy = .relay
        }

        // We will create an application recipient, with callConnection acting as the delegate.
        let recipient = CallConnectionRecipient(delegate: callConnection)

        // Create a call configuration to pass to RingRTC.
        let callConfig = IOSCallConfig(
            callId: callId,
            outBound: isOutgoing,
            recipient: recipient.getWrapper()
        )

        // Create the default media constraints.
        let mediaConstraints = RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: ["DtlsSrtpKeyAgreement": "true"])

        guard let ringRtcCallConnection = ringRtcCreateCallConnection(
            self.ringrtcFactory,
            Unmanaged.passUnretained(callConnection).toOpaque(),
            callConfig,
            ringRtcObserver,
            Unmanaged.passUnretained(configuration).toOpaque(),
            Unmanaged.passUnretained(mediaConstraints).toOpaque()) else {
                // @todo Confirm cleanup of all variables above...

                throw CallConnectionError.ringRtcCreateFailure(description: "ringRtcCreateCallConnection() returned failure")
        }

        Logger.debug("ringRtcCallConnection: \(ringRtcCallConnection)")

        callConnection.ringRtcCallConnection = ringRtcCallConnection

        // We will create all the streams we need while creating the
        // Call Connection, but bear in mind that we *may* want the
        // application to have the flexibility instead.
        callConnection.createAudioSender()
        callConnection.createVideoSender()

        return callConnection
    }

    // MARK: Internal Support Functions

    @objc(callConnectionWithCustomObserver:configuration:constraints:customObserver:)
    public func callConnectionWithCustomObserver(callConnection: CallConnection,
                         withConfiguration configuration: RTCConfiguration,
                             withConstraints constraints: RTCMediaConstraints,
                             withObserver customObserver: UnsafeMutableRawPointer) -> UnsafeMutableRawPointer? {
        return callConnection.initialize(withCustomObserver: customObserver,
                                                    factory: self,
                                              configuration: configuration,
                                                constraints: constraints)
    }
}
