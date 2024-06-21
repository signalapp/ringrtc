# syntax=docker/dockerfile:1.7-labs

#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

FROM ubuntu:22.04 AS build-stage

# Update system packages.
RUN apt-get update \
    && apt upgrade -y \
    && apt-get install -y --no-install-recommends --no-install-suggests curl build-essential ca-certificates protobuf-compiler \
    && update-ca-certificates

SHELL [ "/bin/bash", "-c"]

# Install Rust.
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Since protobuf is a workspace package, we have to stub the entire workspace
WORKDIR /usr/src/ringrtc
COPY --parents ./**/Cargo.toml ./**/Cargo.lock ./**/main.rs ./**/lib.rs ./**/bin/*.rs ./
RUN shopt -s globstar; shopt -s nullglob; \
    for filename in ./**/main.rs; do \
        truncate -s 0 "$filename"; \
    done; \
    for filename in ./**/lib.rs; do \
        truncate -s 0 "$filename"; \
    done; \
    for filename in ./**/bin/*.rs; do \
        truncate -s 0 "$filename"; \
    done
# stub the signaling server, but remove the actual code first
RUN cd call_sim/docker && \
    rm -rf signaling_server && \
    cargo new signaling_server
COPY call_sim/docker/signaling_server/Cargo.* ./call_sim/docker/signaling_server/
RUN cd call_sim/docker/signaling_server && cargo build --release

# Copy the source and build the project normally.
COPY protobuf protobuf
RUN cd protobuf && cargo build --release

COPY call_sim/docker/signaling_server call_sim/docker/signaling_server
RUN cd call_sim/docker/signaling_server && cargo build --release

FROM ubuntu:22.04 AS run-stage

RUN apt-get update \
    && apt upgrade -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build-stage /usr/src/ringrtc/call_sim/docker/signaling_server/target/release/signaling_server /usr/local/bin/

USER nobody:nogroup

# Expose http server access ports to this container.
EXPOSE 8080

ENTRYPOINT ["signaling_server"]
