#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

FROM ubuntu:22.04

RUN apt-get update \
    && apt-get install -y curl iproute2 iperf iptables iputils-ping pip

# Install `tcconfig` so that it can be used as an alternative to using `tc` directly.
RUN pip install tcconfig==0.28.0

COPY src/rust/target/release/call_sim-cli /usr/local/bin/
