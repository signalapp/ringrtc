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

WORKDIR /usr/src

# Create a stub version of the project to cache dependencies.
RUN cargo new signaling_server

WORKDIR /usr/src/signaling_server

COPY Cargo.toml Cargo.lock ./

# Do the initial stub build.
RUN cargo build --release

# Copy the source and build the project normally.
COPY . .

RUN cargo build --release

FROM ubuntu:22.04 AS run-stage

RUN apt-get update \
    && apt upgrade -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build-stage /usr/src/signaling_server/target/release/signaling_server /usr/local/bin/

USER nobody:nogroup

# Expose http server access ports to this container.
EXPOSE 8080

ENTRYPOINT ["signaling_server"]
