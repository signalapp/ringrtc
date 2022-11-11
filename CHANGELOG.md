# Changelog

## v2.21.5

- Group Calls: Improve ring handling

- Group Calls: Update group membership upon unknown media keys 

- Improve display of stats in logs

- Update builds and documentation

- Update Rust

## v2.21.4

- iOS: Add isValidOfferMessage and isValidOpaqueRing to the API

## v2.21.3

- iOS: Allow WebRTC field trials to be set

- Update dependencies, builds

## v2.21.2

- Android: Fix possible crash from AndroidNetworkMonitor

- Electron: Update dependencies (neon mainly)

- Reference signalapp/webrtc@5005b
  - Cherry-pick commits to fix issues

## v2.21.1

- Group Calls: Expose `isHigherResolutionPending` to apps

- Android: Fix race when audio levels change early

- iOS: Set deployment target to 12.2

- Other logging improvements

## v2.21.0

- Update to WebRTC 5005 (m102)

- Allow clients to specify the active speaker's height

- Reference signalapp/webrtc@5005a
  - Add logging for audio device timing

## v2.20.14

- Reference signalapp/webrtc@4896g
  - Windows: Support multi-channel output

## v2.20.13

- Android: Remove audio level debug logging

- Group Calls: Expose decoded video height to apps

- Handle out-of-order IceCandidate and Hangup messages

- Turn off backtraces to stderr by default

## v2.20.12

- Group Calls: Prefer recently received group call rings

- Reduce binary size by dropping unicode support from the regex crate

- Enforce that errors are handled on background tokio runtimes

- Update Android builds
  - Update gradle dependencies
  - Use `-C linker` instead of ndk toolchains

## v2.20.11

- Add support for TURN over TLS

- Android: Add echo likelihood to logs

- Reference signalapp/webrtc@4896f
  - Add support for TURN over TLS
  - Enable echo detection

- Update Rust

- Update builds

## v2.20.10

- Group Calls: Enable audio recording properly

## v2.20.9

- Reference signalapp/webrtc@4896d
  - Have one default port allocator flags instead of two

## v2.20.8

- Reference signalapp/webrtc@4896c
  - Remove bitrate multiplier

- Electron: Add logging to video support

## v2.20.7

- Log PeerConnection ICE gathering errors

- Let rust core enable media (playback and recording), not clients

## v2.20.6

- Prioritize VP9 and H.264 hardware codecs for 1:1 calls

- Add more logging for checking connectivity and group call issues

- Update parse_log.py utility for more debugging

- Reference signalapp/webrtc@4896b
  - Cherry-pick upstream fixes for network crash and iOS audio/logging

- Update Android builds

## v2.20.5

- Fix a deadlock when calling set_network_route

## v2.20.4

- Remove old video frames when re-enabling video

- Use less bandwidth when using TURN relays

- Improve support when developing on M1 chips

- Avoid notifying remote ringing in case of accepted before connected 

- Process remote status events received before the call is accepted

- Android: Allow local video recording to be started while ringing

- Reference signalapp/webrtc@4896a
  - Fix issue with opus frame length for AudioSendStream

- Adjust logging

## v2.20.3

- iOS: Fix mapping of log output

## v2.20.2

- Update to WebRTC 4896 (M100)

- Disable transport-cc for audio

## v2.20.1

- Add VP9 codec support and enable for Android hardware/Electron

- Add state for ConnectingAfterAccepted to fix connect/accept race on caller's end

- Group Calls: Fire peek changed events even if the call is empty

- Reference signalapp/webrtc@4638j
  - Reduce more noise from error/warning logs

- Update dependencies, builds, and ci

## v2.20.0

- Clean up "lite" interfaces

- Add recall support

- Fix typos

- Add WebRTC error and warning logs to RingRTC logging

- Reference signalapp/webrtc@4638i
  - Reduce noise from error/warning logs

## v2.19.2

- Introduce a "lite" part of RingRTC

## v2.19.1

- Android: Add default enum for audio processing

## v2.19.0

- Group Calls: Increase max send bitrate for large calls

- Group Calls: Use v2 frontend api and remove notion of endpoint_id

- Reference signalapp/webrtc@4638h
  - Android: Add Aec3/AecM switch
  - Windows: Workaround for multi-channel input

- Android: Add aec switch and remove legacy default

- Electron: Bubble up more DemuxIds

- Update Rust and dependencies

## v2.18.1

- Fix group call rate constant

- iOS: Fix audio level api for group calls and tests

## v2.18.0

- Update Audio Level API to specify desired interval

- Electron: Use WebCodecs to capture and send video

- Reference signalapp/webrtc@4638f
  - Group Calls: Enable 3rd spatial layer for video

- Update dependencies

## v2.17.2

- Electron: Revert new state and fix issue with prering ended handling

## v2.17.1

- Electron: Fix incoming call notifications for better call history

- Reference signalapp/webrtc@4638e
  - Mac: Fix stereo playout bug

- Update dependencies

## v2.17.0

- Add API to get the incoming and outgoing audio levels

## v2.16.1

- Node: Optimize use of CanvasVideoRenderer.renderVideoFrame

- Node: Update builds and logging

## v2.16.0

- Group Calls: Leave via RTP instead of HTTP

- Group Calls: Don't use DTLS

- Group Calls: Increase default max receive rate

## v2.15.0

- Android: Add audio processing options (to control AEC/NS)

- Android: Improve JNI/Rust interfaces

- Remove legacy Multi-Ring checks and hangup

## v2.14.3

- Avoid handling RTP Data before accepted

- Reference signalapp/webrtc@4638c
  - Port crash fix

## v2.14.2

- Don't terminate a 1:1 call because of transient RTP data error

- Reference signalapp/webrtc@4638b
  - Make it possible to share an APM between PeerConnections (ensures AEC/NS operation)

## v2.14.1

- Desktop: Clear out the incoming video frame to avoid rendering old data

- iOS: Delete the dSYMs out of the built xcframework

## v2.14.0

- Update WebRTC to 4638 (M95)

- Further improvements to WebRTC pointer management

- Replace DataChannel with direct RTP data

- Logging/Testing/Build improvements

## v2.13.6

- Use SetAudioPlayout() function for group calls

## v2.13.5

- Improve how WebRTC pointer is tracked across FFI

- Update Rust

- Update dependencies

- Update builds

## v2.13.4

- Electron: Use Neon's Channel to avoid polling for events/logs

- Desktop: Allow logger to be initialized multiple times

- Enable the use of the SetAudioPlayout() function to start playout after accept

- Reference signalapp/webrtc@4389k
  - Initialize ADM playout before starting

## v2.13.3

- iOS & Android: Pass PeerConnectionFactory down to Rust for group calls

- Desktop: Fix an issue generating device lists on Windows

- Add test client for group calls

- Adjust some interfaces between RingRTC and WebRTC

- Reference signalapp/webrtc@4389j
  - Cleanup iOS interfaces

## v2.13.2

- Desktop: Update local preview source object correctly

- Android: Build Java against the same SDK/NDK that WebRTC uses

## v2.13.1

- Desktop: Add support for auto-ended call timestamps

- Desktop: Formatting and other updates

- Android: Fix signature for new argument

## v2.13.0

- Desktop: Option to use new or default audio device module on Windows

- Reference signalapp/webrtc@4389i
  - Support new Windows ADM

- Desktop: Support glare scenarios

- Request updated membership proof for group calls at least once a day

- Request bitrate constraints for group calls according to BandwidthMode

- Fix PeerConnectionFactory leaks

- iOS: Remove dependency on PromiseKit

- Android: Enable a Hardware AEC blocklist and fix a memory leak

- Android: Native PeerConnectionFactory uses AndroidNetworkMonitor and JavaAudioDeviceModule

## v2.12.0

- Enable ICE continual gathering

- Add signaling for the removal of ICE candidates

- Add notifications for network route changes

- Adjust ringing timeout to 60 seconds

- iOS: Fixes to address resource leaks

- Reference signalapp/webrtc@4389h
  - iOS: AudioSession adjustments for volume issues

- Update builds and documentation

## v2.11.1

- Update Group Ringing feature

## v2.11.0

- Add Group Ringing feature

- Reference signalapp/webrtc@4389f

- Remove DTLS and SDP

## v2.10.8

- Group Calling: Reduce notifications for active speakers

- Android: Modify NDK dependencies and use armv7 instead of arm

- Update logging

## v2.10.7

- iOS: Add support for building for Catalyst

- iOS: Update builds

- Update dependencies

## v2.10.6

- Electron: Use Buffer everywhere we used to use ArrayBuffer

- iOS: Update builds and tests to support M1 iOS simulator

- Update to Rust nightly

## v2.10.5

- Screenshare: Allow screenshare without a camera

## v2.10.4

- Screenshare: Add optimizations

## v2.10.3

- Screenshare: Fix bandwidth for group call

## v2.10.2

- Screenshare: Fix sending of status

## v2.10.1

- Screenshare: Fixes for legacy clients

- Build Fixes: Support older Linux distros and other optimizations

- Reference signalapp/webrtc@4389c

## v2.10.0

- Add Screensharing feature

- Electron: Support alternative target architectures

## v2.9.7

- Electron: Rebuild (no functional changes)

## v2.9.6

- Revert change for shared picture ID in WebRTC

## v2.9.5

- Reference signalapp/webrtc@4389a

- Update dependencies

- Update builds and tests

## v2.9.4

- Add statistics to monitor connection information

- Reference signalapp/webrtc@4183l

- Adjust logging and build issues

## v2.9.3

- Electron: Update neon to use n-api runtime

- CI optimizations and lint improvements

## v2.9.2

- Electron: Update to version 11

- Android: Add setOrientation() API

- Update contributing readme

## v2.9.1

- Electron: Fix Windows build

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
