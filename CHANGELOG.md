# Changelog

## v2.42.0

- Add support for reporting rtc_stats to client application

- Update to webrtc 6261i
  - Support for reporting rtc_stats
  - Enable per-layer PLI for screen sharing

## v2.41.0

- Call links: Add Call Link state to PeekInfo

- Update to webrtc 6261g
  - Update video settings
  - iOS: Match WebRTC acknowledgments filename

- iOS: Update builds and tests

- Update dependencies and documentation

## v2.40.1

- iOS: Raised hands array can be empty

## v2.40.0

- Group Calls: Support multi-recipient message sending

- Group Calls: Update bitrate limits for screen sharing

- Update to webrtc 6261e
  - Update to use Opus 1.5

## v2.39.3

- Update to webrtc 6261d
  - Add receive support for encrypted TOC byte
  - Add logging when select fails

- Add receive support for encrypted TOC byte

- Update dependencies

## v2.39.2

- Group Calls: Apply removal of demux IDs separately

- Log notebook improvements

- Build improvements

- Update to Rust 1.76.0

## v2.39.1

- Call Sim: Add jitter buffer config

- Don't probe when close to the max probe rate

- Group Calls: Synchronize access to last_height_by_demux_id

- Update dependencies

## v2.39.0

- Update to WebRTC m122

- Desktop: Update IceServer fields to be optional

- Add receive support for dependency descriptor to determine unencrypted length

- Group Calls: Handle client_status in sfu.join()

- Call links: Replace update revocation API with an explicit delete API

- Update dependencies

## v2.38.0

- Update to webrtc 6099c
  - Accept list of IceServers for Turn configuration

- Desktop: Accept list of IceServers for Turn configuration

- Enable "First Ready" Turn pruning policy

## v2.37.1

- Update to webrtc 6099b
  - Apply upstream m120 change to JsepTransportController

- Call Sim: Add PESQ and PLC MOS support

## v2.37.0

- Update to WebRTC m120

- Desktop: added_time and speaker_time are not optional

- Desktop: Support installing via npm

- Update dependencies

## v2.36.0

- Use unified plan for group calls

- Update jni crate to 0.21.1

- Desktop: Remove legacy call message fields

## v2.35.0

- Update zkgroup to 0.37.0

- Code Cleanup

- Desktop: Always use the Windows ADM2

- Android: Generate assets/acknowledgments/ringrtc.md as part of build

- Make lints on CI slightly faster

- Build improvements and dependency updates

- Update to Rust 1.74.1

## v2.34.5

- Use unified plan for 1:1 calls

- iOS: Make trivial RemoteDeviceState for IndividualCalls

- iOS: Make isUsingFrontCamera publically readable

- Call Sim: Add deterministic loss handling and lbred test

- Build webrtc using github actions

## v2.34.4

- Fetch build artifacts using a proxy where necessary

- Update to WebRTC m118

## v2.34.3

- Update to webrtc 5845j
  - Add low bitrate redundancy support
  - Lower port allocation step delay
  - Prune TurnPorts on a per-server basis
  - Unregister sink properly when closing

- Call Sim: Improvements for running large test sets

## v2.34.2

- Group Calls: Propagate demux_id to LocalDeviceState

## v2.34.1

- Cleanup logging

- Desktop: Remove device preloading to avoid permission prompt

## v2.34.0

- Group Calls: Add Hand Raise feature

- Electron: Allow ICE server hostname to be set

- Build improvements and dependency updates

## v2.33.0

- Update to webrtc 5845h
  - Add Rust_setIncomingAudioMuted
  - Update libvpx dependency

- Group Calls: Add Reactions feature

- Group Calls: Prevent comfort noise from getting stuck on

- Replace TaskQueueRuntime with Actors

- Call Sim: Speed up chart generation

## v2.32.1

- Desktop: Downgrade dependency for client

## v2.32.0

- Add callback for low upload bandwidth in a video call

- Increase max video receive resolution for desktop

- Update webrtc to 5845f
  - Disable audio and media flow by default
  - Allow configuration of audio jitter buffer max target delay

- iOS: Stop building for Catalyst

- Call links: Add `reset-approvals` to test client

- Update Rust to 1.72.1

- Build improvements and dependency updates

## v2.31.2

- Update webrtc to 5845c
  - Update the hardcoded PulseAudio device name to "Signal Calling"
  - Add more audio control and safe defaults
  - Add accessor for bandwidth estimate

- Update webrtc to 5845d
  - Disable early initialization of recording

- Generate license files for WebRTC builds

- Call Sim: Add test iterations and mos averaging

- Add more audio configuration and control

- Improve builds on GitHub Actions

- Build webrtc on AWS for android, ios, linux, mac

## v2.31.1

- Update tag for build automation

## v2.31.0

- Group Calls: Separate PeekInfo device counts on in/excluding pending devices

- Desktop: Migrate to deviceCountIncluding/ExcludingPendingDevices as well

- Update to WebRTC m116

- Desktop: Use stack arrays for JS arguments rather than vectors

- Build improvements; Support more build automation

- Log improvements

## v2.30.0

- Add JoinState.PENDING, for call link calls with admin approval

- Group Calls: Compute send rates based on devices, not users

- CI: Only run the slow tests on the private repo

- Call Sim: Use a fixed resolution for output video

- Log notebook improvements

## v2.29.1

- Electron: Disable output format limits when screensharing

## v2.29.0

- Call Links: Add Admin Actions support

- Desktop: Adapt video resolutions in 1:1 calls

- Add a Call Simulator for testing

- Reference signalapp/webrtc@5615c
  - Add configuration options to support simulation
  - Support adapting video frames 

- Reference signalapp/webrtc@5615d
  - Configure audio jitter buffer max delay

- Improvements to build scripts for automating WebRTC builds

- Test and logging improvements

## v2.28.1

- Group Calls: Add support for TCP connections

- Call Links: Switch to X-Room-Id header

- Adjust max audio jitter buffer size to support increased packet time

- Test and logging improvements

## v2.28.0

- Call Links: Implement Peek and Join support

- Refactor: BandwidthMode to DataMode

- Android: Fix exception check when throwing an error up to Java

- Improvements to make tests more reliable

## v2.27.0

- Update to WebRTC 5615 (m112)

- Implement Call Link Create/Read/Update APIs

- Set audio packet time to 60ms

- Apply audio encoder configuration in group calls

- ios: Fix video capture size selection

- Refactor HTTP JSON parsing so it's more reusable

- Bump Rust toolchain to nightly-2023-03-17

- Build improvements and dependency updates

## v2.26.4

- Desktop: Stop duplicate MediaStreamTracks

## v2.26.3

- Remove h264 video codec support
  - Reference signalapp/webrtc@5481c

- Disable ANY address ports by default

- Build improvements

## v2.26.2

- Node: Require expected calling message fields

- Log notebook improvements

## v2.26.1

- Revert "Android: Increase max jitter buffer size" (from v2.25.0)

## v2.26.0

- Adjustments to CallId, EraId, RingId and derivations/conversions

- Group Calls: Limit bitrate for the lowest layer

- Reference signalapp/webrtc@5481b
  - VideoAdapter: Fix scaling of very large frames
  - Log more info when video input starts

- Reference signalapp/webrtc@5481a
  - Set inactive timeout to 30s
  - rffi: Set a bandwidth limit on the lowest layer of a group call
  - Allow tcp candidates in group calls

- Log notebook improvements

- Build improvements

## v2.25.2

- Node: Ensure that a frame is fully copied before sending it to WebRTC

- Node: Clean up our eslint config, and fix uncovered issues

- Log stats 2sec into a call, then every 10sec after

- Build improvements

## v2.25.1

- Update to WebRTC 5481 (m110)

- Use default ptime for all bandwidth modes

- Desktop: Add workaround for slow call to enumerateDevices

- Update dependencies (Rust and Electron)

## v2.25.0

- Allow SFU to return multiple ICE candidates (for IPv6 support)

- Android: Add more devices to hardware encoding blocklist

- Android: Increase max jitter buffer size

- Desktop: Initialize call endpoint lazily

- Desktop: Allow explicitly rejecting very tall or very wide frames

- Add cpu statistics to logging

- Reference signalapp/webrtc@5359d
  - Improved logging around network switch
  - Allow TURN ports to be pruned

- CI: Add "Slow Tests" that will run once every night

- Update dependencies, logging, build improvements

## v2.24.0

- Desktop: Get TURN servers after call creation to improve glare handling

- Desktop: Add test cases for glare handling

- Desktop: Set a minimum frame rate for screenshare capture

- Reference signalapp/webrtc@5359c
  - Remove Android API 19 support
  - Cleanup merge diffs
  - Include candidate information for ICE route changes
  - Allow any address ports to be disabled

- Log when the selected ICE candidate pair changes

- Add debuglogs notebook for analyzing logs

- CI: Add builds and tests for all platforms

- Build improvements

## v2.23.1

- Support fetching prebuilds from build-artifacts.signal.org

- Add support for setting WebRTC field trials

- Android: Add support non-vendored NDK

- Update logging, builds

## v2.23.0

- Update to WebRTC 5359 (m108)

- Enable Opus DTX and set default encoding bitrate to 32kbps

- Desktop: Handle failure when entering PiP 

- Desktop: Move builds to NPM

- Update dependencies, builds

## v2.22.0

- Group Calls: Only allow ringing if you are the call creator

- Electron: Add callId to the call ended notification function

- Improve display of stats in logs

- Update dependencies

- Electron: Save debug information when building

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
