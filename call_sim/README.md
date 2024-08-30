# RingRTC Call Simulator
The goal of the Call Simulator is to provide a means to test voice and video calls in a semi-automated way. The calls
themselves are real calls, including all aspects of the WebRTC media stack. However, the test environment in which they
are run is simulated, with network conditions ranging from perfect to terrible. Features include:

- Configure call tests for a variety of scenarios
- Launch calls (1:1 p2p calls only for now)
- Record all artifacts (logs, audio/video output, etc.)
- Generate detailed reports

The simulator is specific to RingRTC and the needs of Signal Messenger. Any other use is not supported.

## Requirements
For best results, running the simulator on an amd64 platform with Ubuntu 20.04 is highly recommended. It can
also be run on macOS, including arm64 machines [see below](#running-on-arm64). 

In addition to the normal requirements for [building RingRTC](../BUILDING.md), you must also have Docker
installed. We recommend that the [Docker Engine](https://docs.docker.com/engine/install/ubuntu/) be installed, not the
Docker Desktop.

## Media Files
To run the simulator, you need to provide a set of media files in an accessible location (by default, this is in
`call_sim/media`). These files are not provided in source code. See the [section below](#creating-media-files) for
more information about creating your own media files.

## Getting Ready
First, make sure that the RingRTC Call Simulator binary is created:

    make call_sim-cli

By default, this build uses the `release` build type and the simulator will look in `src/rust/target/release` for the
binary.

_hint:_ If you are running on a macOS amd64 platform, the binary will not be compatible with the Ubuntu Docker image
that the simulator expects. You will either need to copy the binary over from a Linux/Ubuntu build environment or find
another way to cross-compile it. For arm64, see below.

## Running Tests/Simulations
Now, execute the Call Simulator. Either do this from an IDE or use the following commands:

    cd call_sim
    cargo run --release -- -b -c

Now, wait for the tests to execute. By default, some example tests are run. If they complete successfully, this will
prove that your environment is correctly setup for running the simulator.

After running, you will find a `call_sim/test_results` directory. There, a record of all tests that have been run
is maintained, including all runtime logs and received media.

Each test set has a timestamp for uniqueness and within each set all the groups of individual tests can be found. The
`summary.html` file is a great jumping off point because it includes a summary report of all tests run for a set. Each
test has its own `report.html` along with a collection of runtime artifacts that are useful for further analysis.

If you want to adjust what is run, you can pass the name of one or more test sets defined in
`docker/runner/src/main.rs`, or define your own (by modifying `main.rs` to your own requirements).
As tests are improved and refined, we regularly update the tests provided, but they don't necessarily
represent the breadth of tests we are experimenting with.

Run the simulator with the `--help` option to see more details about the available command line options.

### Notes
- The first time you run the simulator, it could take a while to build the required Docker images
- The `test_results` directory can get quite large if you run lots of tests, especially with video, 
it is a good idea to clean it up occasionally

### Running a Group Call
By default, the call sim will run Direct (1 to 1) calls, potentially starting Turn server containers if enabled. If you
want to run a test as a group call, prefix "group_" to the test name. For example:

    cargo run --release -- -b -c -- group_minimal_example

This will run the test in 2 person group call. Note that the call sim does not start an SFU. Instead, you have to
configure the call sim with an SFU URL and either provide client profile config files (see <repo-root>/config/local for
a template file) or configure an auth key in src/main.rs

## How Does It Work?
The Call Simulator coordinates the tests, it is the _Test Manager_. When run, it executes the tests configured in the
`main.rs` file. The simulator acts as a pseudo-"Docker Compose", running various Docker containers. The first is the
Signaling Server, through which both test and call signaling are routed. Then, two instances of the clients are run,
each in their own container. We call these Client A and Client B. The simulator generally instructs Client A to be
a caller and Client B to be a callee. They both get into a call session using the detailed media configuration set
for the test. The tests last for an arbitrary number of seconds (usually 30, but there are some longer ones), and
during this time the simulator applies various network conditions to the running containers.

At the end of tests, the simulator will run other utilities to analyze artifacts, including recorded audio and video.
Ultimately, the simulator generates reports, a set of html files that provide graphs to visualize what happened during
the call. In addition, the main characteristics of the call are summarized (i.e. averaged) and presented. This includes
the quality scores for audio and video (when applicable). To ascertain the results of any test, the summary reports and
graphs are helpful, as are the recorded audio/video and client logs.

## Tools Used

| Tool             | Description                                                                                                               |
|------------------|---------------------------------------------------------------------------------------------------------------------------|
| coturn           | For tests that require a TURN relay, we use a coturn docker image to provide the functionality.                           |
| Docker           | Each binary runs in a Docker container, and we'll use some other tools provided via docker and associated Rust crates.    |
| ffmpeg           | Used for video conversions.                                                                                               |
| netem            | Part of the Linux traffic control settings for network emulation, used to simulate various network conditions.            |
| signaling-server | A signaling server that relays signaling messages between clients. This is our own tool written in Rust using tonic/gRPC. |
| sox              | Used for audio conversions and visualization.                                                                             |
| tcpdump          | Used to obtain network captures for reporting and analysis.                                                               |
| visqol           | This is used to calculate a MOS (mean opinion score) value to get an estimation of audio quality.                         |
| vmaf             | This is used to calculate a video score to get an estimation of video quality.                                            |

## Creating Media Files
### Audio
Signal uses audio mostly sourced from [ITU P.381](https://www.itu.int/rec/T-REC-P.381#:~:text=P.,interface%20of%20digital%20mobile%20terminals).
These are known to support full-band (20-20kHz frequency range). For now, we use the _silent_ variants. We take
selected talk bursts and create 10, 12, or 30 second clips. Generally, we try to make sure that half of the audio
is silent (sample value of 0) and make sure that there is a nice fade in/out to/from talk bursts. Then, the files
are saved as mono Wav files at 48kHz, 32-bit floating point. These are _unprocessed_ files.

We use `sox` to convert to unprocessed sounds to those suitable for use in the simulator. It does the following:
1. Reads the unprocessed Wav files
2. Filters the sound with a band-pass 20Hz - 20kHz filter
3. Normalizes the sound (-1 dB)
4. Converts to a raw file format at 48kHz, 16-bit signed, stereo (two-channel)

For example, this converts from an unprocessed Wav file:

    docker run --rm -v $(pwd):/work bigpapoo/sox mysox unprocessed.wav -b 16 -c 2 -L -e signed-integer processed.raw --norm=-1 sinc -n 32767 20-20000

You might notice in some of our reference/example tests a variety of sound files. These are described as follows:
- normal_phrasing:   A sequence of talk bursts each separated by a few seconds of silence (50% silence).
- close_phrasing:    Less silence between the talk bursts (35% silence).
- constant_phrasing: Almost no silence between the talk bursts (10% silence).
- sin_tone:          A sine tone at 440Hz. MOS will be meaningless, used to see effects of loss.
- speaker_a:         A sequence of talk bursts inverted yet unique from speaker_b, to avoid double-talk.
- speaker_b:         A sequence of talk bursts inverted yet unique from speaker_a, to avoid double-talk.

The use of speaker_a vs speaker_b is not so important anymore since we disable the use of AEC by default.

### Video
Video support for the Call Simulator is relatively new and still evolving. An Internet search should yield some
reference files. When reading in files, they are preprocessed into YUV format using ffmpeg. The framerate should
be 30fps. For best results, the resolution should be 720p (1280x720). The filename should be formatted carefully,
such as `filename_30fps@1280x720.mp4`.

## Running on arm64
It is possible to run the Call Simulator on arm64 platforms, such as Mac m1/2 machines.

### visqol
By default, the Mac arm64 machines will run docker images built for amd64 using emulation. `visqol` was able to
run, but it was extremely slow. Instead, we now build `visqol` images ourselves. The build itself takes _a long time_.
For this reason, it is recommended that you pre-build the image:

    cd docker/visqol
    docker build -t visqol .

### Building the cli
You might be able to build the `call_sim-cli` on a Mac arm64 machine directly. But for now, you need to cross-compile
on a Linux amd64 machine. These commands have worked and include some dependency updates, which may or may not be
needed:

    sudo apt install crossbuild-essential-arm64
    rustup target add aarch64-unknown-linux-gnu
    ./bin/prepare-workspace unix
    src/webrtc/src/build/linux/sysroot_scripts/install-sysroot.py --arch=arm64
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
    TARGET_ARCH=arm64 OUTPUT_DIR=out_arm ./bin/call_sim-cli --release

The resulting `src/rust/target/release/call_sim-cli` file can be copied to your Mac amd64 machine or run in-place if
using Linux/Ubuntu.
