//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

fn main() {
    tonic_build::compile_protos("proto/call_sim.proto")
        .unwrap_or_else(|e| panic!("Failed to compile protos {:?}", e));
}
