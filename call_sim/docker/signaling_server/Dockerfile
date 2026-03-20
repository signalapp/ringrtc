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

# Install Rust.
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /usr/src/ringrtc

# Minimal workspace root so cargo can resolve protobuf's inherited fields.
RUN cat <<'EOF' > Cargo.toml
[workspace]
resolver = "2"
members = ["protobuf"]

[workspace.package]
version = "0.0.0"
authors = []
EOF

# Copy only the crates signaling_server actually depends on.
COPY protobuf protobuf

# Copy the signaling_server project.
COPY call_sim/docker/signaling_server call_sim/docker/signaling_server

# Build.
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
