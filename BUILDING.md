# Building RingRTC

Building RingRTC depends on a number of prerequisites software
packages.

## Installation prerequisites

### Install Chromium depot_tools

The following is derived from the depot_tools tutorial:
https://commondatastorage.googleapis.com/chrome-infra-docs/flat/depot_tools/docs/html/depot_tools_tutorial.html#_setting_up

    cd <somewhere>
    git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git
    cd depot_tools
    export PATH=<somewhere>/depot_tools:"$PATH"

### Install stgit package

Install the "stacked git" package.

On Debian/Ubuntu run:

    apt install stgit

### Install Rust Components

Install rustup, the rust management system:

    curl https://sh.rustup.rs -sSf | sh

Install rust target support via `rustup`:

    rustup target add \
      arm-linux-androideabi aarch64-linux-android i686-linux-android x86_64-linux-android \
      aarch64-apple-ios armv7-apple-ios armv7s-apple-ios x86_64-apple-ios i386-apple-ios

Install additional components via `cargo`:

    cargo install clippy
    cargo install cargo-lipo
    cargo install cbindgen

### Apple Dev Tools

**Note** currently building WebRTC requires Xcode9+

## Initial Checkout and Branch Selection

The workspace must first be configured for building a specific
platform by running:

    ./bin/prepare-workspace <platform>

where `<platform>` can be one of:
- android
- ios

For example:

    ./bin/prepare-workspace android

This does the following:

1. Checks out the WebRTC branch specified in
   [config/version.sh](config/version.sh).  This step takes a long
   time to download all the WebRTC source components and requires
   several gigabytes of disk space.
1. Applies custom patches from the [patches](patches) directory to the
   webrtc source.
1. Installs additional platform specific details, like the NDK
   toolchains for Android.

### OPTIONAL:  Finding available WebRTC branches

To find available WeBRTC branches follow this guide:
https://www.chromium.org/developers/how-tos/get-the-code/working-with-release-branches

    # Make sure you are in 'ringrtc/src' for this command.

    gclient sync --with_branch_heads

    # OPTIONAL: If that failed, try repeating after running fetch
    git fetch
    gclient sync --with_branch_heads

    # List available branch heads
    git branch -a

    # you should see things like:
    #   remotes/branch-heads/m73
    #   remotes/branch-heads/m74
    #   remotes/branch-heads/m75
    #   ...

    # The part we want is "m74" for example


## Building

### Android

To build an AAR suitable for including in an Android project, run
`./bin/build-aar`, which will produce release and debug builds for all
architectures.  For a debug only build, add `--debug` to the command.
For more options see the output of `--help`.

When the build is complete, the AAR file is available here:

    out/<release|debug>/ringrtc-android<-debug>-<version>.aar
