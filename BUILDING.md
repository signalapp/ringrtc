# Building RingRTC

RingRTC currently supports building for Android on a Linux platform (Ubuntu 20.04 recommended) or iOS on a Mac using Xcode (13.0), and for the host platform as a Node.js module for use in Electron apps.

## Prerequisites

Building RingRTC depends on a number of prerequisite software packages.

### Chromium depot_tools

The following is derived from the depot_tools tutorial: https://commondatastorage.googleapis.com/chrome-infra-docs/flat/depot_tools/docs/html/depot_tools_tutorial.html#_setting_up

    cd <somewhere>
    git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git
    export PATH=<somewhere>/depot_tools:"$PATH"

### Protobuf

The protobuf compiler, protoc, is needed to build RingRTC. Installation is platform specific and can be found [here](https://grpc.io/docs/protoc-installation/).

### Rust Components

Install rustup, the Rust management system:

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

We use a pinned nightly toolchain for official builds, specified by our [rust-toolchain file](https://github.com/signalapp/ringrtc/blob/master/rust-toolchain) ([more information](https://rust-lang.github.io/rustup/overrides.html)).

#### Android

Install Rust target support for Android via `rustup`:

    rustup target add \
      armv7-linux-androideabi aarch64-linux-android i686-linux-android x86_64-linux-android

#### iOS

Install Rust target support for iOS, including compiling the stdlib from source, via `rustup`:

    rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim
    rustup component add rust-src

Install cbindgen via `cargo`:

    cargo install cbindgen

### Electron

Usually the Rust installation installs the correct toolchain for your host. In the case of
Windows, we recommend ensuring that the `msvc` toolchain is installed and used for builds.

### Other Dependencies
#### Android Dependencies

You might need some of these. Of course it is assumed that you have the Android SDK installed,
along with the NDK, LLDB, and SDK Tools options. A properly configured JDK (such as openjdk-11-jdk) is also assumed. You may also need the following (on Ubuntu):

    sudo apt install libglib2.0-dev

#### iOS Dependencies

You might need to change the location of the build tools (this depends on where Xcode is installed):

    sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer

You may also need coreutils if not yet installed:

    brew install coreutils

#### Electron Dependencies

Install the expected version of Node.jswhich can be found in src/node/.nvmrc. You can use [nvm](https://github.com/nvm-sh/nvm) or just manually install the corresponding version. Make sure you have node and npm installed.

Then install Yarn (if not already):

    npm install --global yarn

##### MacOS

Follow the iOS dependencies section above.

##### Windows

For Windows, follow the setup from [here](https://github.com/signalapp/Signal-Desktop-Private/blob/development/CONTRIBUTING.md). Here are some other things that might help with the builds:
- Download and install [git](https://git-scm.com/download/win) (enable symbolic links) and use the Git Bash shell
    - The git config should be:
        - git config --global core.autocrlf false
        - git config --global core.filemode false
        - git config --global branch.autosetuprebase always
        - git config --global core.symlinks true
- Download and install [make](http://gnuwin32.sourceforge.net/packages/make.htm)
- Download and install [Python 2.7](https://www.python.org/downloads/)
    - Install it to a location without spaces (e.g. c:\python27)
- Turn off "Real-time protection" in Windows Security settings during the initial build (WebRTC clones several gigabytes of Google tools)

##### Linux

We currently build using Ubuntu 20.04, but other distributions should work. Here are some other
things that might help with the builds:
- `sudo apt install build-essential git curl wget python python2.7`
- In some cases: `sudo apt install pkg-config`

## Initial Checkout

### Clone

Clone the repo to a working directory:

    git clone https://github.com/signalapp/ringrtc.git

We recommend you fork the repo on GitHub, then clone your fork:

    git clone https://github.com/<USERNAME>/ringrtc.git

You can then add the Signal repo to sync with upstream changes:

    git remote add upstream https://github.com/signalapp/ringrtc.git

## Building

<i>Important: If building the for the first time, it will take a long time to download
WebRTC dependencies and then a long time to build WebRTC and RingRTC.</i>

### Android

To build an AAR suitable for including in an Android project, run:

    make android
    
This will produce release and debug builds for all architectures. The first time you run the build for a particular version, it may ask you to accept license agreements for the Android SDK bundled with WebRTC.

When the build is complete, the AAR file is available here:

    out/<release|debug>/ringrtc-android<-debug>-<version>.aar

### iOS

To build libraries suitable for including in an Xcode project, run:

    make ios
    
This will produce release builds for all architectures.

When the build is complete, the libraries will be available here:

    out/WebRTC.xcframework
    out/libringrtc/ringrtc.h
    out/libringrtc/*/libringrtc.a

The Swift sources in src/ios/SignalRingRTC can then be used to build a framework
that links the appropriate libringrtc.a for each architecture.

### Electron

To build the Node.js module suitable for including in an Electron app, run:

    make electron PLATFORM=<platform> NODEJS_ARCH=<arch>

where platform can be:
- `mac` or `ios` (either of these can be used the MacOS)
- `unix` or `android` (either of these can be used for Linux)
- `windows`

and where the (optional) `NODEJS_ARCH` can be:
- `x64`
- `ia32`
- `arm64`

If no `NODEJS_ARCH` is provided, the build script will default to `x64`. Note that architectures other than `x64` are primarily supported through cross-compilation. That is, compiling for `arm64` on a `x64` machine works, but compiling for `arm64` on an `arm64` machine is not supported on all platforms.

When the build is complete, the library will be available here:

    src/node/build/<platform>/libringrtc-<arch>.node

### CLI test tool

To build the CLI test tool for the host platform, run:

    make cli

When the build is complete, the binary will be available at src/rust/target/debug/cli.
The test tool establishes a call over simulated signaling and media channels. You
should hear echo from the speakers while the tool is running.

Tests might fail if the open file limit is too low. If this is the case, you can increase
the limit in the terminal:

    ulimit -a

If the "open files" value is small, such as 256, try increasing it:

    ulimit -n 2048

## Working with the Code

### Rebuilding

To re-build, do the following:

    make distclean
    make <android|ios|electron|cli>

### iOS Testing

To run tests for iOS, you can use the SignalRingRTC project. You might need to install
the dependencies, at least once:

    cd src/ios/SignalRingRTC
    bundle install
    bundle exec pod install

Some of the tests rely on creating incoming connections, which your system "Firewall Options" may
prevent. All tests should pass if you do not have "Block all incoming connections" on and `xctest`
appears in the list of software allowed to receive incoming connections. If it isn't, you can add it
manually by dragging it in from

    open -R $(xcrun --show-sdk-platform-path --sdk iphonesimulator)/Developer/Library/Xcode/Agents/xctest

### Formatting

We use `rustfmt` to keep the rust code tidy. To run:

    cargo fmt
