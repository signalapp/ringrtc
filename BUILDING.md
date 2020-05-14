# Building RingRTC

RingRTC currently supports building for Android on a Linux platform (Ubuntu 18.04 recommended) or iOS on a Mac using Xcode (11.3 or later).

## Prerequisites

Building RingRTC depends on a number of prerequisite software packages.

### Chromium depot_tools

The following is derived from the depot_tools tutorial: https://commondatastorage.googleapis.com/chrome-infra-docs/flat/depot_tools/docs/html/depot_tools_tutorial.html#_setting_up

    cd <somewhere>
    git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git
    export PATH=<somewhere>/depot_tools:"$PATH"

### Rust Components

Install rustup, the Rust management system:

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

We need to use Rust 1.41.1 for now to maintain support for 32-bit clients.

    rustup toolchain install 1.41.1
    rustup default 1.41.1

#### Android

Install Rust target support for Android via `rustup`:

    rustup target add \
      arm-linux-androideabi aarch64-linux-android i686-linux-android x86_64-linux-android

#### iOS

Install Rust target support for iOS via `rustup`:

    rustup target add \
      aarch64-apple-ios armv7-apple-ios armv7s-apple-ios x86_64-apple-ios i386-apple-ios

Install additional components via `cargo`:

    cargo install cargo-lipo
    cargo install cbindgen

### Other Android Dependencies

You might need some of these. Of course it is assumed that you have the Android SDK installed,
along with the NDK, LLDB, and SDK Tools options. A properly configured JDK (such as openjdk-8-jdk)
is also assumed. You may also need the following (on Ubuntu):

    sudo apt install libglib2.0-dev

### Other iOS Dependencies

You might need to change the location of the build tools (this depends on where Xcode is installed):

    sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer

You may also need coreutils if not yet installed:

    brew install coreutils

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

To build an AAR suitable for including in an Android project, first
setup the gradle file
`${PROJECT_ROOT}/publish/android/local.properties`, specifying the
location of the Android SDK:

    sdk.dir=/path/to/Android/Sdk

To perform the build run:

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

## Working with the Code

### Rebuilding

To re-build, do the following:

    make distclean
    make <android|ios>

### iOS Testing

To run tests for iOS, you can use the SignalRingRTC project. You might need to install
the dependencies, at least once:

    cd src/ios/SignalRingRTC
    bundle install
    bundle exec pod install

### Formatting

We use `rustfmt` to keep the rust code tidy. To install:

    rustup toolchain install nightly-2020-03-15 --force

To format the code, in the `src/rust` directory, run the `format-code` script:

    ./scripts/format-code
