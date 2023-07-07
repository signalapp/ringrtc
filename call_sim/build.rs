//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

fn main() {
    tonic_build::configure()
        .build_server(false)
        .compile(&["proto/call_sim.proto"], &["proto"])
        .expect("Service protos are valid");
}
