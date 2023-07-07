#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

FROM ubuntu:22.04 AS build-stage

RUN apt update \
    && apt upgrade -y

# Install all build dependencies and get bazelisk via npm.
RUN DEBIAN_FRONTEND=noninteractive TZ=Etc/UTC apt install -y --no-install-recommends --no-install-suggests build-essential git libboost-all-dev python3-pip \
    && pip3 install numpy absl-py
RUN apt install -y --no-install-recommends --no-install-suggests npm \
    && npm install -g @bazel/bazelisk

WORKDIR /usr/src

RUN git clone --depth 1 --branch v3.3.3 https://github.com/google/visqol

WORKDIR /usr/src/visqol

ENV USE_BAZEL_VERSION=5.3.2
RUN bazelisk build :visqol -c opt

FROM ubuntu:22.04 AS run-stage

RUN apt-get update \
    && apt upgrade -y \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/visqol

COPY --from=build-stage /usr/src/visqol/bazel-bin/visqol /usr/local/bin/
COPY --from=build-stage /usr/src/visqol/model /usr/src/visqol/model/

ENTRYPOINT ["visqol"]
