//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

fn main() {
    // Explicitly state that by depending on build.rs itself, as recommended.
    println!("cargo:rerun-if-changed=build.rs");

    if cfg!(feature = "signaling") {
        let protos = [
            "protobuf/group_call.proto",
            "protobuf/rtp_data.proto",
            "protobuf/signaling.proto",
        ];

        prost_build::compile_protos(&protos, &["protobuf"]).expect("Protobufs are valid");

        for proto in &protos {
            println!("cargo:rerun-if-changed={}", proto);
        }
    }

    if cfg!(feature = "call_sim") {
        tonic_build::configure()
            .build_client(true)
            .build_server(true)
            .build_transport(true)
            .protoc_arg("--experimental_allow_proto3_optional")
            .compile_protos(&["protobuf/call_sim.proto"], &["protobuf"])
            .expect("call_sim service protobufs are valid")
    }
}
