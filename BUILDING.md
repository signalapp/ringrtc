# Building RingRTC

RingRTC currently supports building for Android on a Linux platform (Ubuntu 24.04
recommended) or iOS on a Mac using Xcode (26.6), and for the host platform as a
Node.js module for use in Electron apps.

## Prerequisites

Building RingRTC depends on a number of prerequisite software packages.

If you want to build WebRTC locally, as opposed to using a prebuilt version,
you'll need to follow some additional steps.

Unfortunately, downloading the source for WebRTC and actually compiling it can
be quite slow and CPU-intensive.

If you wish to use a prebuilt of WebRTC, the WebRTC related dependencies are not
required.

### Chromium depot_tools (to build WebRTC)

The following is derived from
[the `depot_tools` tutorial](https://commondatastorage.googleapis.com/chrome-infra-docs/flat/depot_tools/docs/html/depot_tools_tutorial.html#_setting_up)

    cd <somewhere>
    git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git
    export PATH=<somewhere>/depot_tools:"$PATH"

### Chromium Gitcookies (to build WebRTC)

To sync WebRTC build tools from chromium.googlesource.com, you need to setup a
.gitcookies that will authenticate to chromium.googlesource.com. Go to
[chromium.googlesource.com](https://chromium.googlesource.com/) and sign in
with your Google Account. Accept the terms and click
[generate a password](https://www.googlesource.com/new-password),
then click "Authenticate for all of googlesource.com". Follow the instructions
onscreen to setup .gitcookies.

### Protobuf

The protobuf compiler, protoc, is needed to build RingRTC. Installation is
platform specific and can be found [here](https://protobuf.dev/installation/).

### Rust Components

Install rustup, the Rust management system:

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

We use a pinned toolchain for official builds, specified by our
[rust-toolchain file](https://github.com/signalapp/ringrtc/blob/master/rust-toolchain)
([more information](https://rust-lang.github.io/rustup/overrides.html)).


### Android

Install Rust target support for Android via `rustup`:

    rustup target add \
      armv7-linux-androideabi aarch64-linux-android i686-linux-android x86_64-linux-android

### iOS

Install Rust target support for iOS, including compiling the stdlib from source, via `rustup`:

    rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim
    rustup component add rust-src

Install cbindgen via `cargo`, at the version in `config/tools.properties`:

    . config/tools.properties && cargo install --locked --version "$CBINDGEN_VERSION" cbindgen

### Electron

Usually the Rust installation installs the correct toolchain for your host. In the case of
Windows, we recommend ensuring that the `msvc` toolchain is installed and used for builds.

### Other Dependencies

#### Android Dependencies

You might need some of these. Of course it is assumed that you have the Android
SDK installed, along with the NDK, LLDB, and SDK Tools options. A properly
configured JDK (such as openjdk-17-jdk) is also assumed.

For the SDK, install [Android Studio](https://developer.android.com/studio) and
set up the IDE. For the NDK, set up via the [Android Developer guide](https://developer.android.com/studio/projects/install-ndk).
On Mac, LLDB should be present by default. On other platforms it should be
available via your package manager (e.g. `apt`).  The JDK should be installed
automatically when you install Android Studio, as should the SDK Tools.

You may also need the following (on Ubuntu):

    sudo apt install libglib2.0-dev

#### iOS Dependencies

You might need to change the location of the build tools (this depends on where Xcode is installed):

    sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer

You may also need coreutils if not yet installed:

    brew install coreutils

#### Desktop Dependencies

Install the expected version of Node.js which can be found in src/node/.nvmrc.
You can use [nvm](https://github.com/nvm-sh/nvm) or just manually install the
corresponding version. Make sure you have node and npm installed.

For Desktop builds, one of ringrtc's dependencies relies on `cmake` being
available. This can be installed via some package managers, such as:

    brew install cmake  # MacOS dev machine

If it is not available in your system's package manger, see
https://cmake.org/download/.

##### MacOS

Follow the iOS dependencies section above.

##### Windows

For Windows, follow the setup from [here](https://github.com/signalapp/Signal-Desktop/blob/main/CONTRIBUTING.md).
Here are some other things that might help with the builds:
- Download and install [git](https://git-scm.com/download/win) (enable symbolic links) and use the Git Bash shell
    - The git config should be:
        - git config --global core.autocrlf false
        - git config --global core.filemode false
        - git config --global branch.autosetuprebase always
        - git config --global core.symlinks true
- Download and install [make](http://gnuwin32.sourceforge.net/packages/make.htm)
- Download and install [Python 3](https://www.python.org/downloads/)
    - Install it to a location without spaces (e.g. c:\python3)
- Turn off "Real-time protection" in Windows Security settings during the initial build (WebRTC clones several gigabytes of Google tools)
- Turn on "Developer mode" in Windows settings, or enable SeCreateSymbolicLinkPrivilege for your account

##### Linux

We currently build using Ubuntu 20.04, but other distributions should work. Here are some other
things that might help with the builds:
- `sudo apt install build-essential git curl wget python python3 libpulse-dev protobuf-compiler`
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

### Using a prebuilt WebRTC

To quickly build RingRTC on Linux or MacOS using only the rust toolchain, without
rebuilding WebRTC, you may use a "prebuild" of WebRTC by adding `USE_PREBUILD=1`
to your environment variables when invoking make, though note that, as the
prebuilds are release builds, it is not currently reliable to use a prebuild
while doing a debug build of RingRTC - cargo will look in the wrong place
for the WebRTC build.

If you instead want to use a local build of WebRTC that has already been
created, you may set `BUILD_WHAT=ringrtc`.

By default, the Makefile and similar commands will download and build WebRTC.
<i>Important: If building the for the first time, it will take a long time to download
WebRTC dependencies and then a long time to build WebRTC and RingRTC from scratch.</i>

### Android

To build an AAR suitable for including in an Android project, run:

    make android

This will produce release and debug builds for all architectures. The first
time you run the build for a particular version, it may ask you to accept
license agreements for the Android SDK bundled with WebRTC.

When the build is complete, the AAR file is available here:

    out/gradle/outputs/aar/ringrtc-android-<release|debug>.aar

### iOS

To build libraries suitable for including in an Xcode project, run:

    make ios

This will produce release builds for all architectures.

When the build is complete, the libraries will be available here:

    out/<debug|release>/WebRTC.xcframework
    out/<debug|release>/LibRingRTCFfi.xcframework

With `USE_PREBUILD=1`, only LibRingRTCFfi.xcframework is built.

Apps use the Swift package at the root of the repository, plus a `LibRingRTCFfi` binary target from
the same release's `ringrtc-ios-build-v<version>.xcframework.zip`. To test a local build, use this
checkout as a local package and point that binary target at a copy of
`out/release/LibRingRTCFfi.xcframework`. To also use a local WebRTC, quit Xcode and reopen it with
`RINGRTC_WEBRTC_XCFRAMEWORK=out/release/WebRTC.xcframework` set.

### Desktop

To build the Node.js module suitable for including in an Electron app (e.g. Desktop), run:

    make electron PLATFORM=<platform>

where platform can be `mac`, `unix`, or `windows`.

and where the (optional) `TARGET_ARCH` environment variable can be:
- `x64`
- `ia32`
- `arm64`

If no `TARGET_ARCH` is provided, the build script will default to the host
architecture.

When the build is complete, the library will be available here:

    src/node/build/<platform>/libringrtc-<arch>.node

### CLI test tool

To build the "direct" CLI test tool for the host platform, run:

    make direct

When the build is complete, the binary will be available at target/<debug|release>/direct.
The test tool establishes a call over simulated signaling and media channels. You
should hear echo from the speakers while the tool is running.

Tests might fail if the open file limit is too low. If this is the case, you can increase
the limit in the terminal:

    ulimit -a

If the "open files" value is small, such as 256, try increasing it:

    ulimit -n 2048

## Working with the Code

### iOS Testing

To build and run tests on an iOS simulator:

    bin/run-ios-tests

Extra arguments go to `xcodebuild`, for example
`bin/run-ios-tests -only-testing:SignalRingRTCTests/CallLinkTests`.

To run the tests from Xcode, build `out/release/LibRingRTCFfi.xcframework` for the simulator, quit
Xcode if it is open, and then open the package with `RINGRTC_FFI_XCFRAMEWORK` set:

    bin/build-ios --ringrtc-only --host-sim-only
    RINGRTC_FFI_XCFRAMEWORK=out/release/LibRingRTCFfi.xcframework xed .

Some of the tests rely on creating incoming connections, which your system "Firewall Options" may
prevent. All tests should pass if you do not have "Block all incoming connections" on and `xctest`
appears in the list of software allowed to receive incoming connections. If it isn't, you can add it
manually by dragging it in from

    open -R $(xcrun --show-sdk-platform-path --sdk iphonesimulator)/Developer/Library/Xcode/Agents/xctest

### Formatting

We use `rustfmt` to keep the rust code tidy. To run:

    cargo +nightly fmt
