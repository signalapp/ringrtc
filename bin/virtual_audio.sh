#!/bin/bash

# Copyright 2025 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

set -eou pipefail

usage()
{
  echo "usage: $0  [--setup|--teardown|--play|--stop] [--input-source <name>] [--output-sink <name>] [--input-file <path>] [--input-loops <count>] [--output-file <path>] [--blackhole-path <path>]
    where:
        --setup: Set up the specified devices
        --teardown: Tear down the specified devices.
        --play: Start playing from and recording to specified files and devices.
        --stop: Stop playing and recording to the specified devices.
        --input-source: The name of the source your application will use.
            NOTE: this script may create additional devices with this name as a suffix.
        --output-sink: The name of the device your application will output to.
        --input-file: The name of the file to play to your application (should be a .wav).
        --input-loops: Optional. The number of times to loop |input|. Default 1.
        --output-file: The name of the file to record your application to (should be a .wav).
        --blackhole-path: Optional, macOS only. Path to a checkout of the BlackHole github repo.
            If unspecified, setup will create a temp dir and clone to there.

        -h, --help: Display this usage text.

   Examples:
      # Setup 'my_spiffy_input_source' as an input and 'my_spiffy_output_sink' as an out
      $0 --setup --input-source my_spiffy_input_source --output-sink my_spiffy_output_sink
      # Start playing foo.wav to my_spiffy_input_source
      $0 --play --input-source my_spiffy_input_source --output-sink my_spiffy_output_sink --input-file foo.wav
      # Stop playing to my_spiffy_input_source
      $0 --stop --input-source my_spiffy_input_source --output-sink my_spiffy_output_sink
      # Destroy the specified virtual input devices.
      $0 --teardown --input-source my_spiffy_input_source --output-sink my_spiffy_output_sink

  "
}

TYPE=
INPUT_SOURCE=
INPUT_SINK=
OUTPUT_SINK=
INPUT_FILE=
INPUT_LOOPS=1
OUTPUT_FILE=
BLACKHOLE_PATH=

while [[ $# -gt 0 ]]; do
  case $1 in
    --setup )
      TYPE=setup
      ;;
    --teardown )
      TYPE=teardown
      ;;
    --play )
      TYPE=play
      ;;
    --stop )
      TYPE=stop
      ;;
    --input-source )
      INPUT_SOURCE="$2"
      shift
      ;;
    --output-sink )
      OUTPUT_SINK="$2"
      shift
      ;;
    --input-file )
      INPUT_FILE="$2"
      shift
      ;;
    --output-file )
      OUTPUT_FILE="$2"
      shift
      ;;
    --input-loops )
      INPUT_LOOPS="$2"
      shift
      ;;
    --blackhole-path )
      BLACKHOLE_PATH="$2"
      shift
      ;;
    -h | --help )
      usage
      exit
      ;;
    * )
      echo "Did not recognize flag $1"
      usage
      exit 1
  esac
  shift
done

setup_macos()
{
  CLEANUP_GIT=
  if [ -n "$BLACKHOLE_PATH" ]; then
    cd "$BLACKHOLE_PATH"
  else
    BLACKHOLE_PATH=$(mktemp -d -t virtual_audio)
    CLEANUP_GIT=1

    cd "$BLACKHOLE_PATH"
    git clone https://github.com/ExistentialAudio/BlackHole.git --revision=v0.6.1
    cd BlackHole
  fi

  xcodebuild CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO \
    -project BlackHole.xcodeproj -configuration Release -target BlackHole \
    CONFIGURATION_BUILD_DIR=build \
    GCC_PREPROCESSOR_DEFINITIONS='kNumber_Of_Channels=2 kDriver_Name=\"'"$INPUT_SOURCE"'\" kDevice_Name=\"'"$INPUT_SOURCE"'\"'
  echo "Copying $INPUT_SOURCE driver to /Library/Audio/Plug-Ins/HAL: will require sudo"
  sudo cp -r build/BlackHole.driver /Library/Audio/Plug-Ins/HAL/"$INPUT_SOURCE".driver

  xcodebuild CODE_SIGN_IDENTITY="" CODE_SIGNING_REQUIRED=NO \
    -project BlackHole.xcodeproj -configuration Release -target BlackHole \
    CONFIGURATION_BUILD_DIR=build \
    GCC_PREPROCESSOR_DEFINITIONS='kNumber_Of_Channels=2 kDriver_Name=\"'"$OUTPUT_SINK"'\" kDevice_Name=\"'"$OUTPUT_SINK"'\"'
  echo "Copying $OUTPUT_SINK driver to /Library/Audio/Plug-Ins/HAL: will require sudo"
  sudo cp -r build/BlackHole.driver /Library/Audio/Plug-Ins/HAL/"$OUTPUT_SINK".driver

  echo "Restarting coreaudiod; audio output may glitch"
  sudo killall -9 coreaudiod

  if [ -n "$CLEANUP_GIT" ]; then
    echo "cleanup tempdir $BLACKHOLE_PATH"
    rm -fr "$BLACKHOLE_PATH"
  fi
}

teardown_macos()
{
  echo "Removing $INPUT_SOURCE from install dir; will require sudo"
  sudo rm -r /Library/Audio/Plug-Ins/HAL/"$INPUT_SOURCE".driver

  echo "Removing $OUTPUT_SINK from install dir; will require sudo"
  sudo rm -r /Library/Audio/Plug-Ins/HAL/"$OUTPUT_SINK".driver

  echo "Restarting coreaudiod; audio output may glitch"
  sudo killall -9 coreaudiod
}

play_macos()
{
  if [ -n "$INPUT_FILE" ] && [ -e "$INPUT_FILE" ]; then
    # "repeat" is the number of times to repeat, not the total number of plays
    nohup sox -q "$INPUT_FILE" -t coreaudio "$INPUT_SOURCE" repeat $((INPUT_LOOPS - 1)) &
  fi
  if [ -n "$OUTPUT_FILE" ]; then
    nohup sox -q -t coreaudio "$OUTPUT_SINK" "$OUTPUT_FILE" &
  fi
}

stop_macos()
{
  # Kill the record process
  pkill -f "sox.*${OUTPUT_SINK}" || true
  # Kill the play process
  pkill -f "sox.*${INPUT_SOURCE}" || true
}

setup_linux()
{
  pactl load-module module-null-sink sink_name="${INPUT_SINK}" \
    format=s16 rate=48000 channels=2 > /dev/null  # ignore module ID
  # Use this as a dummy module to turn the monitor source, which Signal
  # Desktop ignores, into a non-monitor source
  pactl load-module module-remap-source source_name="${INPUT_SOURCE}" \
    source_properties=device.description="${INPUT_SOURCE}" \
    format=s16 rate=48000 channels=2 master="${INPUT_SINK}".monitor \
    master_channel_map=front-left,front-right \
    channel_map=front-left,front-right remix=false > /dev/null  # ignore ID
  pactl load-module module-null-sink sink_name="${OUTPUT_SINK}" \
    sink_properties=device.description="${OUTPUT_SINK}" \
    format=s16 rate=48000 channels=2 > /dev/null # ignore ID
}

teardown_linux()
{
  # safe to ignore failures here
  pactl unload-module module-null-sink || true
  pactl unload-module module-remap-source || true
}

play_linux()
{
  if [ -n "$INPUT_FILE" ] && [ -e "$INPUT_FILE" ]; then
    (
      for _ in $(seq 1 "${INPUT_LOOPS}"); do
        paplay --device="${INPUT_SINK}" "$INPUT_FILE" || break  # Exit early if requested
      done
    ) &
  fi
  if [ -n "$OUTPUT_FILE" ]; then
    parecord --format=s16 --rate=48000 --channels=2 \
      --device="${OUTPUT_SINK}".monitor "${OUTPUT_FILE}" &
  fi
}

stop_linux()
{
  # Kill the recording process
  pkill --full "parecord.*${OUTPUT_SINK}.monitor" || true
  # Kill the play process
  pkill --full "paplay.*${INPUT_SINK}" || true
}


if [ -z "$TYPE" ]; then
  echo "One of --setup, --teardown, --play, --stop is required"
  usage
  exit 1
fi

if [ -z "$INPUT_SOURCE" ]; then
  echo "--input-source is required"
  usage
  exit 1
fi

INPUT_SINK="sink_for_${INPUT_SOURCE}"

if [ -z "$OUTPUT_SINK" ]; then
  echo "--output-sink is required"
  usage
  exit 1
fi

UNAME=$(uname)

if [ "$UNAME" = "Darwin" ]; then
  if ! which -s sox ; then
    echo "sox is required to play audio"
    exit 1
  fi
  if [ "$TYPE" = "setup" ]; then
    setup_macos
  elif [ "$TYPE" = "teardown" ]; then
    teardown_macos
  elif [ "$TYPE" = "play" ]; then
    if [ -z "$INPUT_FILE" ]; then
      echo "--input-file was not specified with --play; assuming silence"
    fi
    if [ -z "$OUTPUT_FILE" ]; then
      echo "--output-file was not specified with --play; will not record"
    fi
    play_macos
  elif [ "$TYPE" = "stop" ]; then
    stop_macos
  fi
elif [ "$UNAME" = "Linux" ]; then
  if [ "$TYPE" = "setup" ]; then
    setup_linux
  elif [ "$TYPE" = "teardown" ]; then
    teardown_linux
  elif [ "$TYPE" = "play" ]; then
    if [ -z "$INPUT_FILE" ]; then
      echo "--input-file was not specified with --play; assuming silence"
    fi
    if [ -z "$OUTPUT_FILE" ]; then
      echo "--output-file was not specified with --play; will not record"
    fi
    play_linux
  elif [ "$TYPE" = "stop" ]; then
    stop_linux
  fi
else
  echo "$UNAME" is not yet supported
  exit 1
fi
