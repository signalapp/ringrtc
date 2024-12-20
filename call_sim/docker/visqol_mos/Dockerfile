#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

FROM ubuntu:22.04 AS build-stage

RUN apt update \
    && apt upgrade -y

# Install all build dependencies and get bazelisk via npm.
RUN DEBIAN_FRONTEND=noninteractive TZ=Etc/UTC apt install -y --no-install-recommends --no-install-suggests build-essential git curl unzip libboost-all-dev python3-pip \
    && pip3 install numpy absl-py
RUN apt install -y --no-install-recommends --no-install-suggests npm \
    && npm install -g @bazel/bazelisk

ENV USE_BAZEL_VERSION=5.3.2

WORKDIR /usr/src

RUN git clone --depth 1 --branch v3.3.3 https://github.com/google/visqol

WORKDIR /usr/src/visqol

# Build fix: Override the tensorflow runtime package since it was moved.
RUN mkdir -p /usr/src/visqol/overrides/tf_runtime \
    && curl -L -o /usr/src/visqol/overrides/tf_runtime.tar.gz https://github.com/tensorflow/runtime/archive/4ce3e4da2e21ae4dfcee9366415e55f408c884ec.tar.gz \
    && tar -xzf /usr/src/visqol/overrides/tf_runtime.tar.gz -C /usr/src/visqol/overrides/tf_runtime --strip-components=1

# Build fix: Override the XNNPACK package since it was moved.
RUN mkdir -p /usr/src/visqol/overrides/XNNPACK \
    && curl -L -o /usr/src/visqol/overrides/XNNPACK.zip https://github.com/google/XNNPACK/archive/e8f74a9763aa36559980a0c2f37f587794995622.zip \
    && unzip -q /usr/src/visqol/overrides/XNNPACK.zip -d /usr/src/visqol/overrides/XNNPACK \
    && mv /usr/src/visqol/overrides/XNNPACK/XNNPACK-*/* /usr/src/visqol/overrides/XNNPACK/ \
    && rm -rf /usr/src/visqol/overrides/XNNPACK/XNNPACK-*

# Build fix: Use a current armadillo package since the old ones were archived.
RUN sed -i 's|^    sha256 = "d856ea58c18998997bcae6689784d2d3eeb5daf1379d569fddc277fe046a996b"|    sha256 = "248e2535fc092add6cb7dea94fc86ae1c463bda39e46fd82d2a7165c1c197dff"|' WORKSPACE && \
    sed -i 's|^    strip_prefix = "armadillo-9.860.2"|    strip_prefix = "armadillo-14.0.2"|' WORKSPACE && \
    sed -i 's|^    urls = \["http://sourceforge.net/projects/arma/files/armadillo-9.860.2.tar.xz"\]|    urls = \["http://sourceforge.net/projects/arma/files/armadillo-14.0.2.tar.xz"\]|' WORKSPACE

RUN bazelisk build :visqol -c opt \
    --override_repository=tf_runtime=/usr/src/visqol/overrides/tf_runtime \
    --override_repository=XNNPACK=/usr/src/visqol/overrides/XNNPACK

FROM ubuntu:22.04 AS run-stage

RUN apt-get update \
    && apt upgrade -y \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/visqol

COPY --from=build-stage /usr/src/visqol/bazel-bin/visqol /usr/local/bin/
COPY --from=build-stage /usr/src/visqol/model /usr/src/visqol/model/

ENTRYPOINT ["visqol"]
