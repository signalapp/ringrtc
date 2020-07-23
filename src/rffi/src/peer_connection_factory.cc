/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#include "api/create_peerconnection_factory.h"
#include "api/task_queue/default_task_queue_factory.h"
#include "api/audio_codecs/builtin_audio_decoder_factory.h"
#include "api/audio_codecs/builtin_audio_encoder_factory.h"
#include "api/video_codecs/builtin_video_decoder_factory.h"
#include "api/video_codecs/builtin_video_encoder_factory.h"
#include "modules/audio_mixer/audio_mixer_impl.h"
#include "pc/peer_connection_factory.h"
#include "rffi/api/media.h"
#include "rffi/api/peer_connection_factory.h"
#include "rffi/api/injectable_network.h"
#include "rtc_base/logging.h"
#include "rtc_base/log_sinks.h"
#include "rtc_base/rtc_certificate_generator.h"

namespace webrtc {
namespace rffi {

class PeerConnectionFactoryWithOwnedThreads
    : public PeerConnectionFactoryOwner {
 public:
  static rtc::scoped_refptr<PeerConnectionFactoryWithOwnedThreads> Create(bool use_injectable_network) {
    // Creating a PeerConnectionFactory is a little complex.  To make sure we're doing it right, we read several examples:
    // Android SDK:
    //  https://cs.chromium.org/chromium/src/third_party/webrtc/sdk/android/src/jni/pc/peer_connection_factory.cc
    // iOS SDK:
    //  https://cs.chromium.org/chromium/src/third_party/webrtc/sdk/objc/api/peerconnection/RTCPeerConnectionFactory.mm
    // Chromium:
    //  https://cs.chromium.org/chromium/src/third_party/blink/renderer/modules/peerconnection/peer_connection_dependency_factory.cc
    // Default:
    //  https://cs.chromium.org/chromium/src/third_party/webrtc/api/create_peerconnection_factory.cc?q=CreateModularPeerConnectionFactory%5C(&dr=C&l=40
    // Others:
    //  https://cs.chromium.org/chromium/src/remoting/protocol/webrtc_transport.cc?l=246
    //  https://cs.chromium.org/chromium/src/third_party/webrtc/examples/peerconnection/client/conductor.cc?q=CreatePeerConnectionFactory%5C(&l=133&dr=C
    //  https://cs.chromium.org/chromium/src/third_party/webrtc/examples/unityplugin/simple_peer_connection.cc?q=CreatePeerConnectionFactory%5C(&dr=C&l=131
    //  https://cs.chromium.org/chromium/src/third_party/webrtc/examples/objcnativeapi/objc/objc_call_client.mm?q=CreateModularPeerConnectionFactory%5C(&dr=C&l=104
    //  https://cs.chromium.org/chromium/src/third_party/webrtc/examples/androidnativeapi/jni/android_call_client.cc?q=CreatePeerConnectionFactory%5C(&dr=C&l=141

    auto network_thread = CreateAndStartNetworkThread("Network-Thread");
    auto worker_thread = CreateAndStartNonNetworkThread("Worker-Thread");
    auto signaling_thread = CreateAndStartNonNetworkThread("Signaling-Thread");
    std::unique_ptr<InjectableNetwork> injectable_network;
    if (use_injectable_network) {
      injectable_network = CreateInjectableNetwork(network_thread.get());
    }
    auto factory = CreatePeerConnectionFactory(
        network_thread.get(),
        worker_thread.get(),
        signaling_thread.get(),
        nullptr,  // default_adm build if we don't set it
        CreateBuiltinAudioEncoderFactory(),
        CreateBuiltinAudioDecoderFactory(),
        CreateBuiltinVideoEncoderFactory(),
        CreateBuiltinVideoDecoderFactory(),
        AudioMixerImpl::Create(),
        nullptr  // audio_processing built if we don't set it
    );
    auto owner = new rtc::RefCountedObject<PeerConnectionFactoryWithOwnedThreads>(
        std::move(factory),
        std::move(network_thread),
        std::move(worker_thread),
        std::move(signaling_thread),
        std::move(injectable_network));
    owner->AddRef();
    return owner;
  }

  ~PeerConnectionFactoryWithOwnedThreads() override {
      RTC_LOG(LS_INFO) << "~PeerConnectionFactoryWithOwnedThreads()";
  }

  PeerConnectionFactoryInterface* peer_connection_factory() override {
    return factory_.get();
  }

  rffi::InjectableNetwork* injectable_network() override {
    return injectable_network_.get();
  }

 protected:
  PeerConnectionFactoryWithOwnedThreads(
      rtc::scoped_refptr<PeerConnectionFactoryInterface> factory,
      std::unique_ptr<rtc::Thread> owned_network_thread,
      std::unique_ptr<rtc::Thread> owned_worker_thread,
      std::unique_ptr<rtc::Thread> owned_signaling_thread,
      std::unique_ptr<rffi::InjectableNetwork> injectable_network) :
    owned_network_thread_(std::move(owned_network_thread)),
    owned_worker_thread_(std::move(owned_worker_thread)),
    owned_signaling_thread_(std::move(owned_signaling_thread)),
    injectable_network_(std::move(injectable_network)),
    factory_(std::move(factory)) {
  }

 private:
  static std::unique_ptr<rtc::Thread> CreateAndStartNetworkThread(std::string name) {
    std::unique_ptr<rtc::Thread> thread = rtc::Thread::CreateWithSocketServer();
    thread->SetName(name, nullptr);
    thread->Start();
    return thread;
  }

  static std::unique_ptr<rtc::Thread> CreateAndStartNonNetworkThread(std::string name) {
    std::unique_ptr<rtc::Thread> thread = rtc::Thread::Create();
    thread->SetName(name, nullptr);
    thread->Start();
    return thread;
  }

  const std::unique_ptr<rtc::Thread> owned_network_thread_;
  const std::unique_ptr<rtc::Thread> owned_worker_thread_;
  const std::unique_ptr<rtc::Thread> owned_signaling_thread_;
  std::unique_ptr<rffi::InjectableNetwork> injectable_network_;
  const rtc::scoped_refptr<PeerConnectionFactoryInterface> factory_;
};

RUSTEXPORT PeerConnectionFactoryOwner* Rust_createPeerConnectionFactory(bool use_injectable_network) {
  auto factory_owner = PeerConnectionFactoryWithOwnedThreads::Create(use_injectable_network);
  return factory_owner.release();
}

RUSTEXPORT PeerConnectionInterface* Rust_createPeerConnection(
    PeerConnectionFactoryOwner* factory_owner,
    PeerConnectionObserver* observer,
    rtc::RTCCertificate* certificate,
    bool hide_ip,
    RffiIceServer ice_server,
    webrtc::AudioTrackInterface* outgoing_audio_track,
    webrtc::VideoTrackSourceInterface* outgoing_video_source,
    bool enable_dtls,
    bool enable_rtp_data_channel) {
  auto factory = factory_owner->peer_connection_factory();

  PeerConnectionInterface::RTCConfiguration config;
  config.bundle_policy = PeerConnectionInterface::kBundlePolicyMaxBundle;
  config.rtcp_mux_policy = PeerConnectionInterface::kRtcpMuxPolicyRequire;
  config.tcp_candidate_policy = PeerConnectionInterface::kTcpCandidatePolicyDisabled;
  if (hide_ip) {
    config.type = PeerConnectionInterface::kRelay;
  }
  config.certificates.push_back(certificate);
  if (ice_server.urls_size > 0) {
    webrtc::PeerConnectionInterface::IceServer rtc_ice_server;
    rtc_ice_server.username = std::string(ice_server.username);
    rtc_ice_server.password = std::string(ice_server.password);
    for (size_t i = 0; i < ice_server.urls_size; i++) {
      rtc_ice_server.urls.push_back(std::string(ice_server.urls[i]));
    }
    config.servers.push_back(rtc_ice_server);
  }

  config.enable_dtls_srtp = enable_dtls;
  config.enable_rtp_data_channel = enable_rtp_data_channel;

  PeerConnectionDependencies deps(observer);
  if (factory_owner->injectable_network()) {
    deps.allocator = factory_owner->injectable_network()->CreatePortAllocator();
  }
  rtc::scoped_refptr<PeerConnectionInterface> pc = factory->CreatePeerConnection(
    config, std::move(deps));

  // We use an arbitrary stream_id because existing apps want a MediaStream to pop out.
  auto stream_id = "s";
  std::vector<std::string> stream_ids;
  stream_ids.push_back(stream_id);

  if (outgoing_audio_track) {
    auto result = pc->AddTrack(outgoing_audio_track, stream_ids);
    if (!result.ok()) {
      RTC_LOG(LS_ERROR) << "Failed to PeerConnection::AddTrack(audio)";
    }
  }

  if (outgoing_video_source) {
    auto outgoing_video_track =
      factory->CreateVideoTrack("v", outgoing_video_source);
    if (outgoing_video_track) {
      auto result = pc->AddTrack(outgoing_video_track, stream_ids);
      if (!result.ok()) {
        RTC_LOG(LS_ERROR) << "Failed to PeerConnection::AddTrack(video)";
      }
    } else {
      RTC_LOG(LS_ERROR) << "Failed to PeerConnectionFactory::CreateVideoTrack";
    }
  }

  return pc.release();
}

RUSTEXPORT webrtc::rffi::InjectableNetwork* Rust_getInjectableNetwork(
    PeerConnectionFactoryOwner* factory_owner) {
  return factory_owner->injectable_network(); 
}

RUSTEXPORT AudioTrackInterface* Rust_createAudioTrack(
    PeerConnectionFactoryOwner* factory_owner) {
  auto factory = factory_owner->peer_connection_factory();

  cricket::AudioOptions options;
  auto source = factory->CreateAudioSource(options);
  auto track = factory->CreateAudioTrack("a", source);
  return track.release();
}

RUSTEXPORT VideoTrackSourceInterface* Rust_createVideoSource(
    PeerConnectionFactoryOwner* factory_owner) {
  auto source = new rtc::RefCountedObject<webrtc::rffi::VideoSource>();
  source->AddRef();
  return source;
}

// This could technically be in its own file because it's not part of PeerConnectionFactory,
// but this is a convenient place to put it.
RUSTEXPORT rtc::RTCCertificate* Rust_generateCertificate() {
  rtc::KeyParams params;  // default is ECDSA
  absl::optional<uint64_t> expires_ms;  // default is to never expire?
  auto cert = rtc::RTCCertificateGenerator::GenerateCertificate(params, expires_ms);
  return cert.release();
}

} // namespace rffi
} // namespace webrtc
