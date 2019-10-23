# Building RingRTC

RingRTC currently supports building for Android on a Linux platform (Ubuntu 18.04 recommended) or iOS on a Mac using Xcode (10.1 or later).

## Prerequisites

Building RingRTC depends on a number of prerequisite software packages.

### Chromium depot_tools

The following is derived from the depot_tools tutorial: https://commondatastorage.googleapis.com/chrome-infra-docs/flat/depot_tools/docs/html/depot_tools_tutorial.html#_setting_up

    cd <somewhere>
    git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git
    cd depot_tools
    export PATH=<somewhere>/depot_tools:"$PATH"

### stgit

Install the "stacked git" package.

#### Android

On Ubuntu run:

    apt install stgit

#### iOS

    brew install stgit

### Rust Components

Install rustup, the rust management system:

    curl https://sh.rustup.rs -sSf | sh

Install additional rustup components:

    rustup component add clippy

Install additional components via `cargo`:

    cargo install cargo-lipo
    cargo install cbindgen

Install rust target support via `rustup`:

#### Android

    rustup target add \
      arm-linux-androideabi aarch64-linux-android i686-linux-android x86_64-linux-android

#### ios

    rustup target add \
      aarch64-apple-ios armv7-apple-ios armv7s-apple-ios x86_64-apple-ios i386-apple-ios

### Other iOS Dependencies

    xcode-select --install
    brew install coreutils

After the initial checkout (described below), you will need to install Bundle dependencies at least once:

    cd src/ios
    bundle install

## Initial Checkout

### Clone

Clone the repo to a working directory:

    git clone https://github.com/signalapp/ringrtc.git

We recommend you fork the repo on GitHub, then clone your fork:

    git clone https://github.com/<USERNAME>/ringrtc.git

You can then add the Signal repo to sync with upstream changes:

    git remote add upstream https://github.com/signalapp/ringrtc.git

## Building

### Android

To build an AAR suitable for including in an Android project, run:

    make android
    
This will produce release and debug builds for all architectures.

When the build is complete, the AAR file is available here:

    out/<release|debug>/ringrtc-android<-debug>-<version>.aar

### iOS

To build frameworks suitable for including in an Xcode project, run:

    make ios
    
This will produce release and debug builds for all architectures.

When the build is complete, the frameworks will be available here:

    out/SignalRingRTC.framework
    out/WebRTC.framework

Dynamic symbol files are also available in the `out/` directory for each framework.
