//
//  Copyright (c) 2019 Open Whisper Systems. All rights reserved.
//

#import <Foundation/Foundation.h>
#import <SignalCoreKit/OWSLogs.h>

// @note We had to set Packaging/Defines Module to Yes in Build Settings.
#import <SignalRingRTC/SignalRingRTC-Swift.h>

// At the end of the day, RingRTC (in Rust) doesn't care what specific
// object is being tracked in the application. So, RingRTC will call this
// app-specific function to create a PeerConnection, which happens to be
// of type CallConnection which itself subclasses RTCPeerConnection.
//
// Ultimately, since in the current scheme we already have the object
// 'shell' for CallConnection here in the app space, we will return the
// low level WebRTC PeerConnection pointer back to RingRTC, because it
// is needed by the rffi interface.
void *appCreatePeerConnection(void *appFactory,
                              void *appCallConnection,
                              void *rtcConfig,
                              void *rtcConstraints,
                              void *customObserver) {
    OWSLogDebug(@"appCreatePeerConnection appFactory = %p", appFactory);
    OWSLogDebug(@"appCreatePeerConnection appCallConnection = %p", appCallConnection);
    OWSLogDebug(@"appCreatePeerConnection customObserver = %p", customObserver);

    CallConnectionFactory *factory = (__bridge CallConnectionFactory *)appFactory;
    CallConnection *callConnection = (__bridge CallConnection *)appCallConnection;
    RTCConfiguration *configuration = (__bridge RTCConfiguration *)rtcConfig;
    RTCMediaConstraints *constraints = (__bridge RTCMediaConstraints *)rtcConstraints;

    return (void *)([factory callConnectionWithCustomObserver:callConnection
                                                configuration:configuration
                                                  constraints:constraints
                                               customObserver:customObserver]);
}

void *appCreateStreamFromNative(const void *appCallConnection,
                                void *nativeStream) {
    OWSLogDebug(@"appCreateStreamFromNative appCallConnection = %p", appCallConnection);
    OWSLogDebug(@"appCreateStreamFromNative nativeStream = %p", nativeStream);

    if (nativeStream != NULL) {
        CallConnection *callConnection = (__bridge CallConnection *)appCallConnection;
        return (__bridge void *)([callConnection createStreamFromNative:nativeStream]);
    } else {
        return NULL;
    }
}

void appReleaseStream(const void *appCallConnection,
                      void *appStream) {
    OWSLogDebug(@"appReleaseStream appCallConnection = %p", appCallConnection);
    OWSLogDebug(@"appReleaseStream appStream = %p", appStream);

    // @note It seems that the RTCMediaStream gets released in
    // some other way, possibly when calling close on the
    // RTCPeerConnection... If we don't disable this code, we
    // see a crash with EXC_BAD_ACCESS.

    //CallConnection *callConnection = (__bridge CallConnection *)appCallConnection;
    //RTCMediaStream *mediaStream = (__bridge RTCMediaStream *)appStream;

    //[callConnection releaseStream:mediaStream];
}
