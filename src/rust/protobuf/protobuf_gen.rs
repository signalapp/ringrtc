//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

fn main() {
    println!("Compiling protobufs ...");

    let proto_files = [
        "protobuf/rtp_data.proto",
        "protobuf/signaling.proto",
        "protobuf/group_call.proto",
    ];

    let output = "src/protobuf";

    let mut prost_build = prost_build::Config::new();
    prost_build.out_dir(output);
    prost_build
        .compile_protos(&proto_files, &["protobuf"])
        .unwrap();

    println!("Success: protobufs generated in {}.", output);
}
