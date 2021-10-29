//
// Copyright 2020-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};

use log::info;

use ringrtc::webrtc::peer_connection_factory;
use ringrtc::{
    common::{
        actor::{Actor, Stopper},
        HttpMethod, HttpResponse,
    },
    core::{
        call_mutex::CallMutex,
        group_call::{
            self, ClientId, ConnectionState, DemuxId, EndReason, JoinState, RemoteDeviceState,
            RemoteDevicesChangedReason, UserId,
        },
        http_client,
        sfu_client::SfuClient,
    },
    protobuf,
    webrtc::{
        media::{VideoFrame, VideoFrameMetadata, VideoSink, VideoTrack},
        peer_connection_factory::PeerConnectionFactory,
    },
};

#[derive(Clone)]
struct HttpClient {
    actor: Actor<()>,
}

impl HttpClient {
    fn start() -> Self {
        Self {
            actor: Actor::start(Stopper::new(), |_| Ok(())).unwrap(),
        }
    }
}

impl http_client::HttpClient for HttpClient {
    fn make_request(
        &self,
        url: String,
        method: HttpMethod,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
        on_response: Box<dyn FnOnce(Option<HttpResponse>) + Send>,
    ) {
        self.actor.send(move |_| {
            let mut tls_config = rustls::ClientConfig::new();
            tls_config
                .dangerous()
                .set_certificate_verifier(Arc::new(ServerCertVerifier {}));
            let agent = ureq::builder().tls_config(Arc::new(tls_config)).build();

            let mut request = match method {
                HttpMethod::Get => agent.get(&url),
                HttpMethod::Put => agent.put(&url),
                HttpMethod::Delete => agent.delete(&url),
                HttpMethod::Post => agent.post(&url),
            };
            for (key, value) in headers.iter() {
                request = request.set(key, value);
            }
            let request_result = match body {
                Some(body) => request.send_bytes(&body),
                None => request.call(),
            };
            match request_result {
                Ok(response) => {
                    let status_code = response.status();
                    let mut body = Vec::new();
                    if response.into_reader().read_to_end(&mut body).is_ok() {
                        on_response(Some(HttpResponse { status_code, body }));
                    } else {
                        on_response(None);
                    }
                }
                Err(ureq::Error::Status(status_code, response)) => {
                    let mut body = Vec::new();
                    if response.into_reader().read_to_end(&mut body).is_ok() {
                        on_response(Some(HttpResponse { status_code, body }));
                    } else {
                        on_response(None);
                    }
                }
                Err(ureq::Error::Transport(_)) => {
                    on_response(None);
                }
            }
        });
    }
}

struct ServerCertVerifier {}

impl rustls::ServerCertVerifier for ServerCertVerifier {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        _presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef,
        _ocsp_response: &[u8],
    ) -> core::result::Result<rustls::ServerCertVerified, rustls::TLSError> {
        Ok(rustls::ServerCertVerified::assertion())
    }
}
#[derive(Clone, Default)]
struct Observer {
    remote_devices: Arc<Mutex<Vec<group_call::RemoteDeviceState>>>,
    last_frame_metadata_by_track_id: Arc<Mutex<HashMap<u32, VideoFrameMetadata>>>,
}

impl group_call::Observer for Observer {
    fn request_membership_proof(&self, _client_id: ClientId) {
        // Should be done before starting
    }

    fn request_group_members(&self, _client_id: ClientId) {
        // Done via handle_peek_changed
    }

    fn handle_connection_state_changed(
        &self,
        _client_id: ClientId,
        connection_state: ConnectionState,
    ) {
        info!("Connection state changed to {:?}", connection_state);
    }

    fn handle_join_state_changed(&self, _client_id: ClientId, join_state: JoinState) {
        info!("Join state changed to {:?}", join_state);
    }

    fn handle_remote_devices_changed(
        &self,
        _client_id: ClientId,
        remote_devices: &[RemoteDeviceState],
        _reason: RemoteDevicesChangedReason,
    ) {
        info!("Remote devices changed to {:?}", remote_devices);
        *self.remote_devices.lock().unwrap() = remote_devices.to_vec();
    }

    fn handle_peek_changed(
        &self,
        _client_id: ClientId,
        joined_members: &[UserId],
        creator: Option<UserId>,
        era_id: Option<&str>,
        max_devices: Option<u32>,
        device_count: u32,
    ) {
        info!(
            "Peek info changed to joined: {:?} creator: {:?}, era: {:?} devices: {:?}/{:?}",
            joined_members, creator, era_id, device_count, max_devices
        );
    }

    fn send_signaling_message(
        &mut self,
        _recipient_id: UserId,
        _message: ringrtc::protobuf::signaling::CallMessage,
        _urgency: ringrtc::core::group_call::SignalingMessageUrgency,
    ) {
        // This isn't going to work :(.  Better turn of frame crypto.
    }

    fn handle_incoming_video_track(
        &mut self,
        _client_id: ClientId,
        sender_demux_id: DemuxId,
        _incoming_video_track: VideoTrack,
    ) {
        info!("Got a video track for {}", sender_demux_id);
    }

    fn handle_ended(&self, _client_id: ClientId, reason: EndReason) {
        info!("Ended with reason {:?}", reason);
    }

    fn send_signaling_message_to_group(
        &mut self,
        _group: group_call::GroupId,
        _message: protobuf::signaling::CallMessage,
        _urgency: group_call::SignalingMessageUrgency,
    ) {
        unimplemented!()
    }

    fn handle_network_route_changed(
        &self,
        _client_id: ClientId,
        _network_route: ringrtc::webrtc::peer_connection_observer::NetworkRoute,
    ) {
        // ignore
    }
}

impl VideoSink for Observer {
    fn on_video_frame(&self, track_id: u32, frame: VideoFrame) {
        self.last_frame_metadata_by_track_id
            .lock()
            .unwrap()
            .insert(track_id, frame.metadata());
    }

    fn box_clone(&self) -> Box<dyn VideoSink> {
        Box::new(self.clone())
    }
}

struct Log;

static LOG: Log = Log;

impl log::Log for Log {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            println!("{} - {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let url = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("https://sfu.voip.signal.org");
    let membership_proof = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("757365725f6964:67726f75705f6964:1:"); // Hex of "user_id:group_id:timestamp:" with empty MAC

    log::set_logger(&LOG).expect("set logger");
    log::set_max_level(log::LevelFilter::Info);
    ringrtc::webrtc::logging::set_logger(log::LevelFilter::Info);

    let group_id = b"Test Group".to_vec();
    let http_client: HttpClient = HttpClient::start();
    let sfu_client = SfuClient::new(Box::new(http_client), url.to_string());
    let observer = Observer::default();
    let config = peer_connection_factory::Config {
        use_injectable_network: false,
        ..Default::default()
    };
    let peer_connection_factory = PeerConnectionFactory::new(config).unwrap();
    let outgoing_audio_track = peer_connection_factory
        .create_outgoing_audio_track()
        .unwrap();
    let outgoing_video_source = peer_connection_factory
        .create_outgoing_video_source()
        .unwrap();
    let outgoing_video_track = peer_connection_factory
        .create_outgoing_video_track(&outgoing_video_source)
        .unwrap();
    let busy = Arc::new(CallMutex::new(false, "busy"));
    let self_uuid = Arc::new(CallMutex::new(None, "self_uuid"));
    let client = group_call::Client::start(
        group_id,
        1,
        Box::new(sfu_client),
        Box::new(observer.clone()),
        busy,
        self_uuid,
        None,
        outgoing_audio_track,
        Some(outgoing_video_track.clone()),
        Some(Box::new(observer.clone())),
        None,
    )
    .unwrap();

    client.set_membership_proof(membership_proof.as_bytes().to_vec());
    client.connect();
    client.join();
    outgoing_video_track.set_enabled(true);

    std::thread::spawn(move || {
        for index in 0u64.. {
            let width = 1280;
            let height = 720;
            let rgba_data: Vec<u8> = (0..(width * height * 4))
                .map(|i: u32| i.wrapping_add(index as u32) as u8)
                .collect();
            outgoing_video_source.push_frame(VideoFrame::from_rgba(width, height, &rgba_data));
            std::thread::sleep(std::time::Duration::from_secs_f32(1.0 / 30.0));
        }
    });

    let mut request_big_next_time = true;
    std::thread::sleep(std::time::Duration::from_secs(1));
    loop {
        let (width, height) = if request_big_next_time {
            (10000, 10000)
        } else {
            (1, 1)
        };
        request_big_next_time = !request_big_next_time;
        let requests = observer
            .remote_devices
            .lock()
            .unwrap()
            .iter()
            .map(|remote| {
                group_call::VideoRequest {
                    demux_id: remote.demux_id,
                    width,
                    height,
                    framerate: None, // Unrestrained
                }
            })
            .collect();
        info!("Request video of size {}x{}", width, height);
        info!("Requests: {:?}", requests);
        info!(
            "Current videos: {}",
            observer
                .last_frame_metadata_by_track_id
                .lock()
                .unwrap()
                .len()
        );
        for (track_id, metadata) in observer
            .last_frame_metadata_by_track_id
            .lock()
            .unwrap()
            .iter()
        {
            info!("  {} {:?}", track_id, metadata);
        }
        client.request_video(requests);
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}
