# Changelog

## v2.9.0

- Add very low bandwidth support for audio

- Remove SCTP

- Update documentation

## v2.8.10

- Android: Fix JNI out of memory issues for large groups

## v2.8.9

- Android: Fix memory issues for Direct Calling

- Electron: Fix issue where camera was not released

## v2.8.8

- iOS: Fix issue when ending a Group Call

## v2.8.7

- Group Calling: Fix issue with video resolution requests

## v2.8.6

- Update Group Calling feature

- Reference signalapp/webrtc@4183h

## v2.8.5

- Android: Improve stability for Group Calling

## v2.8.4

- Update Group Calling feature

## v2.8.3

- Update Group Calling feature

## v2.8.2

- Update Group Calling feature

- Android: Add more devices to hardware encoder blacklist

- Reference signalapp/webrtc@4183g

## v2.8.1

- Electron: Fix video track setting

## v2.8.0

- Add Group Calling feature

- Reference signalapp/webrtc@4183f

- Update Rust dependencies

- Update builds and documentation

## v2.7.4

- Electron: Fix debug build

## v2.7.3

- Refactor calling code (non-functional improvements)

- Update opus codec settings

- Update builds and documentation

## v2.7.2

- Electron: Expose more message types

## v2.7.1

- Reference signalapp/webrtc@4183a
  - Electron: Should prevent early microphone access

- Electron: Do not stretch video if different resolution

## v2.7.0

- Update Rust dependencies

- Implement "V4" protocol with protobufs; deprecate SDP

- Electron: Improve logging and handling of device selection

## v2.6.0

- Reference signalapp/webrtc@4183

- Implement "V3" protocol; deprecate DTLS

- Fix offer-busy handling and support better glare experience

- Electron: Fix issue when sending busy would end current call

## v2.5.2

- Electron: Mac minimum sdk and os set to 10.10

## v2.5.1

- Electron: Improve device selection on Windows

- Fix message queue issue

## v2.5.0

- Disable processing of incoming RTP before incoming call is accepted

- Electron: A/V device selection support

- Implement low bandwidth mode support

## v2.4.3

- iOS: Update video support

## v2.4.2

- Reference signalapp/webrtc@4147d

## v2.4.1

- Fixes for release

## v2.4.0

- Reference signalapp/webrtc@4147b

- Implement data channel support over RTP; deprecate SCTP

- Add audio statistics logging

- Minor fixes and improvements

## v2.3.1

- Fix for call request support

- Fix to ensure hangups sent

## v2.3.0

- Reference signalapp/webrtc@4103

- Add support for call request permissions

## v2.2.0

- Reference signalapp/webrtc@4044g

- iOS: Remove 32-bit support, require 11.0 target

## v2.1.1

- Reference signalapp/webrtc@4044f

## v2.1.0

- Implement native interface

- Reference signalapp/webrtc@4044e

- Minor API improvements (call, proceed, receivedOffer)

## v2.0.3

- Android: Use video sink for remote video stream

## v2.0.2

- Reference signalapp/webrtc@4044d

## v2.0.1

- Reference signalapp/webrtc@4044c
  - Fixes a call forking bug
  - Improves connectivity using PORTALLOCATOR_ENABLE_ANY_ADDRESS_PORTS
  - Cherry picked updates from WebRTC

- Disable TURN port pruning

- Fix glare handling before connection

## v2.0.0

- Add Multi-Ring feature

- Android: Fix video encoder crash on some devices

- Update build documentation

- Update Rust dependencies

## v1.3.1

- Fix issue preventing some calls from ringing

## v1.3.0

- Update build documentation

- Reference signalapp/webrtc@4044

## v1.2.0

- Move to vendored WebRTC at signalapp/webrtc

- Reference signalapp/webrtc@3987, includes cherry picked updates from WebRTC 4044

## v1.1.0

- Disable unused audio codecs and RTP header extensions

- Adjust settings and logging

- iOS: Minor optimizations

## v1.0.2

- Cherry pick updates from WebRTC 4044

## v1.0.1

- Android: improve logging

## v1.0.0

- Add Call Manager component

## v0.3.3

- Update WebRTC to 3987

- Update Rust dependencies

- iOS: build system improvements

## v0.3.2

- iOS: Fix iOS 13 issue with camera capture

## v0.3.1

- Android: Filter list of cameras when switching cameras

## v0.3.0

- Update WebRTC to m79

- Android: Improve WebRTC debug logging

## v0.2.0

- Improve logging on Android

- Build system improvements

## v0.1.9

- Make termination a two-phase close and dispose operation

## v0.1.8

- Improve logging on Android

- Patch WebRTC M78 for AudioRecord regression

## v0.1.7

- Add integration tests

- Build system fixes and clean up

## v0.1.6

- Android: Use an application supplied logging object

## v0.1.5

- Update WebRTC to m78

- Add integration tests

- Build system fixes and clean up

## v0.1.4

- Update Makefile targets for 'clean' and 'distclean'

- Simplify the IceReconnecting logic

- Remove non-critical DataChannel error callbacks

## v0.1.3

- Add IceReconnectingState

## v0.1.2

- iOS Support

- Update WebRTC to m77

## v0.1.1

- Initial Release

- Based on WebRTC release m76
