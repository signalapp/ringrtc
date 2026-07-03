#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

FROM ubuntu:26.04

RUN apt-get update \
    && apt-get install -y curl \
        iproute2 \
        iperf \
        iptables \
        iputils-ping \
        protobuf-compiler \
        libpulse-dev \
        linux-perf \
        build-essential \
        pulseaudio-utils \
        libpulse0 \
        pipewire \
        dbus \
        pipewire-pulse \
        wireplumber \
        psmisc \
        dbus-user-session \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    &&  . ~/.cargo/env \
    && cargo install --features=bin addr2line && cargo install inferno

RUN curl https://raw.githubusercontent.com/torvalds/linux/refs/heads/master/tools/perf/perf-archive.sh > /usr/local/bin/perf-archive && chmod +x /usr/local/bin/perf-archive

ENV XDG_RUNTIME_DIR=/tmp
ENV DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/bus

COPY call_sim/docker/ringrtc/wireplumber-no-hardware.conf /etc/wireplumber/wireplumber.conf.d/90-sim-no-hardware.conf

COPY target/release/call_sim-cli /usr/local/bin/
