#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

FROM ubuntu:24.04

RUN apt-get update \
    && apt-get install -y curl iproute2 iperf iptables iputils-ping protobuf-compiler libpulse-dev linux-tools-generic linux-tools-common build-essential \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    &&  . ~/.cargo/env \
    && cargo install --features=bin addr2line

COPY target/release/call_sim-cli /usr/local/bin/
