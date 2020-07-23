//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

fn main() {
    println!("Compiling protobufs ...");

    let proto_files = ["protobuf/data_channel.proto", "protobuf/signaling.proto"];

    let output = "src/protobuf";

    let mut prost_build = prost_build::Config::new();
    prost_build.out_dir(output);
    prost_build
        .compile_protos(&proto_files, &["protobuf"])
        .unwrap();

    println!("Success: protobufs generated in {}.", output);
}
