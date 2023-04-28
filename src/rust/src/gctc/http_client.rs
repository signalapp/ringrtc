//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{io::Read, sync::Arc};

use ringrtc::{
    common::actor::{Actor, Stopper},
    lite::http,
};

#[derive(Clone)]
pub struct HttpClient {
    actor: Actor<()>,
}

impl HttpClient {
    pub fn start() -> Self {
        Self {
            actor: Actor::start(Stopper::new(), |_| Ok(())).unwrap(),
        }
    }
}

impl http::Client for HttpClient {
    fn send_request(&self, request: http::Request, response_callback: http::ResponseCallback) {
        let http::Request {
            method,
            url,
            headers,
            body,
        } = request;

        self.actor.send(move |_| {
            let mut tls_config = rustls::client::ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth();
            tls_config
                .dangerous()
                .set_certificate_verifier(Arc::new(ServerCertVerifier {}));
            let agent = ureq::builder().tls_config(Arc::new(tls_config)).build();

            let mut request = match method {
                http::Method::Get => agent.get(&url),
                http::Method::Put => agent.put(&url),
                http::Method::Delete => agent.delete(&url),
                http::Method::Post => agent.post(&url),
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
                        response_callback(Some(http::Response {
                            status: status_code.into(),
                            body,
                        }));
                    } else {
                        response_callback(None);
                    }
                }
                Err(ureq::Error::Status(status_code, response)) => {
                    let mut body = Vec::new();
                    if response.into_reader().read_to_end(&mut body).is_ok() {
                        response_callback(Some(http::Response {
                            status: status_code.into(),
                            body,
                        }));
                    } else {
                        response_callback(None);
                    }
                }
                Err(ureq::Error::Transport(_)) => {
                    response_callback(None);
                }
            }
        });
    }
}

struct ServerCertVerifier {}

impl rustls::client::ServerCertVerifier for ServerCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}
