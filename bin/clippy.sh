#!/bin/bash

set -e

# shellcheck source=bin/env.sh
. "$(dirname "$0")"/env.sh

cd "${PROJECT_DIR}"

echo "==== Clippy ===="
cargo clippy --workspace --all-targets -- -D warnings
echo "==== Clippy ===="

echo "==== Clippy (with sim) ===="
cargo clippy --all-targets --features sim -- -D warnings
echo "==== Clippy (with sim) ===="

echo "==== Clippy (Android & iOS) ===="
cargo clippy --package ringrtc --target aarch64-linux-android --target aarch64-apple-ios -- -D warnings
echo "==== Clippy (Android & iOS) ===="

echo "==== Clippy (Electron) ===="
cargo clippy --package ringrtc --features electron,virtual_audio -- -D warnings
echo "==== Clippy (Electron) ===="

echo "==== Clippy (group_call) ===="
cargo clippy --package ringrtc --bin group_call --features native,sim_http -- -D warnings
echo "==== Clippy (group_call) ===="

echo "==== Clippy (call_link) ===="
cargo clippy --package ringrtc --bin call_link --features uuid,sim,sim_http,rand/chacha -- -D warnings
echo "==== Clippy (call_link) ===="

echo "==== Clippy (call_sim-cli) ===="
cargo clippy --package ringrtc --bin call_sim-cli --features call_sim -- -D warnings
echo "==== Clippy (call_sim-cli) ===="

echo "==== Clippy (call_sim) ===="
cargo clippy --package call_sim -- -D warnings
echo "==== Clippy (call_sim) ===="

echo "==== Clippy (mrp) ===="
cargo clippy --package mrp -- -D warnings
echo "==== Clippy (mrp) ===="

echo "==== Clippy (protobuf) ===="
cargo clippy --package protobuf --features call_sim -- -D warnings
echo "==== Clippy (protobuf) ===="

echo "==== Clippy (signaling_server) ===="
pushd call_sim/docker/signaling_server
cargo clippy -- -D warnings
popd
echo "==== Clippy (signaling_server) ===="

echo "==== Clippy (direct) ===="
cargo clippy --package ringrtc --bin direct --features=direct
echo "==== Clippy (direct) ===="

