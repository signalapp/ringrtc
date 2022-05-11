#!/usr/bin/env python3

#
# Copyright 2022 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# To run: 
#
#  python3 parse_log.py 1.log 2.log ...
#
# To typecheck:
#
#  mypy parse_log.py

from enum import Enum
import re
import sys
from typing import Union, Optional, Iterator, NamedTuple, TypeVar, Iterable, List, Dict

class Duration(NamedTuple):
    secs: float

    def __repr__(self):
        return f"{self.secs:.2f}s"

class Instant(NamedTuple):
    secs: float

    def __repr__(self):
        return f"{self.secs:.2f}s"

    def __sub__(after, before) -> Duration:
        return Duration(after.secs - before.secs)

def parse_timestamp(unparsed: str) -> Instant:
    (hours, mins, secs) = (float(x) for x in unparsed.split(":")[:3])
    secs = (((60 * hours) + mins) * 60) + secs
    return Instant(secs)

class NetworkAdapterType(Enum):
    UNKNOWN = 0
    WIFI = 1
    CELL = 2

def parse_network_adapter_type_name(unparsed: str) -> NetworkAdapterType:
    if unparsed == "Wifi":
        return NetworkAdapterType.WIFI
    elif unparsed == "Cellular":
        return NetworkAdapterType.CELL
    else:
        return NetworkAdapterType.UNKNOWN

class DataSize(NamedTuple):
    bits: int

    @classmethod
    def from_bytes(cls, bytes: int):
        return DataSize(bytes * 8)

    def __repr__(self):
        return f"{self.bits}bits"

    def __truediv__(self, duration):
        return DataRate(int(self.bits/duration.secs))

class DataRate(NamedTuple):
    bps: int

    def __repr__(self):
        return f"{self.bps/1000:.0f}kbps"

    def kbps(self) -> float:
        return self.bps/1000.0

def parse_bps(unparsed: str) -> DataRate:
    return DataRate(int(unparsed))

TIMESTAMP_PATTERN_STR = ".*(\d\d:\d\d:\d\d.\d+).*"

OUTGOING_CALL_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "API:create_outgoing_call.*")

class OutgoingCall(NamedTuple):
    time: Instant

def parse_outgoing_call(line: str) -> Optional[OutgoingCall]:
    match = OUTGOING_CALL_PATTERN.match(line)
    if match:
        (timestamp, ) = match.groups()
        time = parse_timestamp(timestamp)
        return OutgoingCall(time)
    else:
        return None

INCOMING_CALL_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "API:received_offer.*")

class IncomingCall(NamedTuple):
    time: Instant

def parse_incoming_call(line: str) -> Optional[IncomingCall]:
    match = INCOMING_CALL_PATTERN.match(line)
    if match:
        (timestamp, ) = match.groups()
        time = parse_timestamp(timestamp)
        return IncomingCall(time)
    else:
        return None

CALL_START_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "app -> cm: proceed.*REDACTED_HEX:...(.*?) ]")

class CallStart(NamedTuple):
    time: Instant
    call_id: str

def parse_call_start(line: str) -> Optional[CallStart]:
    match = CALL_START_PATTERN.match(line)
    if match:
        (timestamp, call_id) = match.groups()
        time = parse_timestamp(timestamp)
        return CallStart(time, call_id)
    else:
        return None

CALL_STATE_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "on_connection_observer_event.*StateChanged[(](.*)[)].*")

class CallState(NamedTuple):
    time: Instant
    state: str

def parse_call_state(line: str) -> Optional[CallState]:
    match = CALL_STATE_PATTERN.match(line)
    if match:
        (timestamp, state) = match.groups()
        time = parse_timestamp(timestamp)
        return CallState(time, state)
    else:
        return None


CONNECTION_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "create_connection.*remote_device_id: (\d+).*")

class Connection(NamedTuple):
    time: Instant
    remote_device_id: str

def parse_connection(line: str) -> Optional[Connection]:
    match = CONNECTION_PATTERN.match(line)
    if match:
        (timestamp, remote_device_id) = match.groups()
        time = parse_timestamp(timestamp)
        return Connection(time, remote_device_id)
    else:
        return None


LOCAL_ICE_CANDIDATE_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "Local ICE candidate:.*typ ([a-z]+).*")

class LocalIceCandidate(NamedTuple):
    time: Instant
    type: str

def parse_local_ice_candidate(line: str) -> Optional[LocalIceCandidate]:
    match = LOCAL_ICE_CANDIDATE_PATTERN.match(line)
    if match:
        (timestamp, type) = match.groups()
        time = parse_timestamp(timestamp)
        return LocalIceCandidate(time, type)
    else:
        return None

REMOTE_ICE_CANDIDATE_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "Remote ICE candidate:.*typ ([a-z]+).*")

class RemoteIceCandidate(NamedTuple):
    time: Instant
    type: str

def parse_remote_ice_candidate(line: str) -> Optional[RemoteIceCandidate]:
    match = REMOTE_ICE_CANDIDATE_PATTERN.match(line)
    if match:
        (timestamp, type) = match.groups()
        time = parse_timestamp(timestamp)
        return RemoteIceCandidate(time, type)
    else:
        return None

ICE_CONNECTED_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "IceConnected.*")

class IceConnected(NamedTuple):
    time: Instant

def parse_ice_connected(line: str) -> Optional[IceConnected]:
    match = ICE_CONNECTED_PATTERN.match(line)
    if match:
        (timestamp,) = match.groups()
        time = parse_timestamp(timestamp)
        return IceConnected(time)
    else:
        return None

NETWORK_ROUTE_CHANGE_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "rtc -> conn: ice_network_route_change\(NetworkRoute { local_adapter_type: ([A-Za-z]+).*")

class NetworkRouteChange(NamedTuple):
    time: Instant
    adapter_type: NetworkAdapterType

def parse_network_route_change(line: str) -> Optional[NetworkRouteChange]:
    match = NETWORK_ROUTE_CHANGE_PATTERN.match(line)
    if match:
        (timestamp, adapter_type_name) = match.groups()
        time = parse_timestamp(timestamp)
        adapter_type = parse_network_adapter_type_name(adapter_type_name)
        return NetworkRouteChange(time, adapter_type)
    else:
        return None

SEND_RATE_CHANGE_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "ringrtc_stats!,connection,.*?,.*?,(\d+).*")

class SendRateChange(NamedTuple):
    time: Instant
    send_rate: DataRate

def parse_send_rate_change(line: str) -> Optional[SendRateChange]:
    match = SEND_RATE_CHANGE_PATTERN.match(line)
    if match:
        (timestamp, send_rate_bps) = match.groups()
        time = parse_timestamp(timestamp)
        send_rate = parse_bps(send_rate_bps)
        return SendRateChange(time, send_rate)
    else:
        return None

AUDIO_RECEIVE_STATS_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "ringrtc_stats!,audio,recv,(\d+?),(.*?),(.*?),(.*?),(.*?),(.*?),(.*?),(.*?),(.*?)")

class AudioReceiveStats(NamedTuple):
    time: Instant
    ssrc: int
    packets_received: int
    packets_lost: int
    bytes_received: int
    jitter: float
    frames_decoded: int
    total_decode_time: float
    audio_level: float
    total_audio_energy: str

def parse_audio_receive_stats(line: str) -> Optional[AudioReceiveStats]:
    match = AUDIO_RECEIVE_STATS_PATTERN.match(line)
    if match:
        (timestamp, ssrc_str, packets_received_str, packets_lost_str, bytes_received_str, jitter_str, frames_decoded_str, total_decode_time_str, audio_level_str, total_audio_energy_str) = match.groups()
        time = parse_timestamp(timestamp)
        ssrc = int(ssrc_str)
        packets_received = int(packets_received_str)
        packets_lost = int(packets_lost_str)
        bytes_received = int(bytes_received_str)
        jitter = float(jitter_str)
        frames_decoded = int(frames_decoded_str)
        total_decode_time = float(total_decode_time_str)
        audio_level = float(audio_level_str)
        total_audio_energy = total_audio_energy_str
        return AudioReceiveStats(time, ssrc, packets_received, packets_lost, bytes_received, jitter, frames_decoded, total_decode_time, audio_level, total_audio_energy)
    else:
        return None

VIDEO_RECEIVE_STATS_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "ringrtc_stats!,video,recv,(\d+?),(.*?),(.*?),(.*?),(.*?),(.*?),(.*?),(.*?),(.*?),(.*)")

class VideoReceiveStats(NamedTuple):
    time: Instant
    ssrc: int
    packets_received: int
    packets_lost: int
    packets_repaired: int
    bytes_received: int
    frames_decoded: int
    key_frames_decoded: int
    total_decode_time: float
    frame_width: int
    frame_height: int

def parse_video_receive_stats(line: str) -> Optional[VideoReceiveStats]:
    match = VIDEO_RECEIVE_STATS_PATTERN.match(line)
    if match:
        (timestamp, ssrc_str, packets_received_str, packets_lost_str, packets_repaired_str, bytes_received_str, frames_decoded_str, key_frames_decoded_str, total_decode_time_str, frame_width_str, frame_height_str) = match.groups()
        time = parse_timestamp(timestamp)
        ssrc = int(ssrc_str)
        packets_received = int(packets_received_str)
        packets_lost = int(packets_lost_str)
        packets_repaired = int(packets_repaired_str)
        bytes_received = int(bytes_received_str)
        frames_decoded = int(frames_decoded_str)
        key_frames_decoded = int(frames_decoded_str)
        total_decode_time = float(total_decode_time_str)
        frame_width = int(frame_width_str)
        frame_height = int(frame_height_str)
        return VideoReceiveStats(time, ssrc, packets_received, packets_lost, packets_repaired, bytes_received, frames_decoded, key_frames_decoded, total_decode_time, frame_width, frame_height)
    else:
        return None


LOCAL_HANGUP_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "app -> cm: hangup.*")

class LocalHangup(NamedTuple):
    time: Instant

def parse_local_hangup(line: str) -> Optional[LocalHangup]:
    match = LOCAL_HANGUP_PATTERN.match(line)
    if match:
        (timestamp,) = match.groups()
        time = parse_timestamp(timestamp)
        return LocalHangup(time)
    else:
        return None


RECEIVED_HANGUP_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "ReceivedHangup, device: (.*?) hangup: (.*?)[)]")

class ReceivedHangup(NamedTuple):
    time: Instant
    remote_device_id: str
    type: str

def parse_received_hangup(line: str) -> Optional[ReceivedHangup]:
    match = RECEIVED_HANGUP_PATTERN.match(line)
    if match:
        (timestamp, remote_device_id, type) = match.groups()
        time = parse_timestamp(timestamp)
        return ReceivedHangup(time, remote_device_id, type)
    else:
        return None


CALL_END_PATTERN = re.compile(TIMESTAMP_PATTERN_STR + "ringrtc::core::call.*terminate_call.*")
 
class CallEnded(NamedTuple):
    time: Instant

def parse_call_end(line: str) -> Optional[CallEnded]:
    match = CALL_END_PATTERN.match(line)
    if match:
        (timestamp, ) = match.groups()
        time = parse_timestamp(timestamp)
        return CallEnded(time)
    else:
        return None

Event = Union[OutgoingCall, IncomingCall, CallStart, CallState, Connection, LocalIceCandidate, RemoteIceCandidate, IceConnected, NetworkRouteChange, SendRateChange, AudioReceiveStats, VideoReceiveStats, LocalHangup, ReceivedHangup, CallEnded]
def parse_events(lines: Iterable[str]) -> Iterator[Event]:
    for line in lines:
        for parse in [parse_outgoing_call, parse_incoming_call, parse_call_start, parse_call_state, parse_connection, parse_local_ice_candidate, parse_remote_ice_candidate, parse_ice_connected, parse_network_route_change, parse_send_rate_change, parse_audio_receive_stats, parse_video_receive_stats, parse_local_hangup, parse_received_hangup, parse_call_end]:
            event = parse(line)
            if event is not None:
                yield event

class NetworkRoutePeriod(NamedTuple):
    adapter_type: NetworkAdapterType
    start: Instant
    send_rates: List[DataRate]

    def pretty(self, end: Instant):
        duration = end - self.start
        return f"{self.adapter_type} for {duration}; rates: {self.send_rates}"

def print_stats(audio_receive_stats_by_ssrc: Optional[Dict[int, List[AudioReceiveStats]]], video_receive_stats_by_ssrc: Optional[Dict[int, List[VideoReceiveStats]]]):
    if audio_receive_stats_by_ssrc:
        print_audio_stats(audio_receive_stats_by_ssrc)

    if video_receive_stats_by_ssrc:
        print_video_stats(video_receive_stats_by_ssrc)

def print_audio_stats(stats_by_ssrc: Dict[int, List[AudioReceiveStats]]):
    print(f"Audio Receive Stats by SSRC:")
    for (ssrc, stats) in stats_by_ssrc.items():
        print(f" from SSRC {ssrc}:")
        last_sample = None
        for sample in stats:
            loss_percent = "?"
            if sample.packets_received > 0:
                loss_percent = int(sample.packets_lost*100.0/(sample.packets_received + sample.packets_lost))
            kbps_since_last_time = "?"
            if last_sample:
                bytes_received_since_last_time = DataSize.from_bytes(sample.bytes_received - last_sample.bytes_received)
                duration_since_last_time = sample.time - last_sample.time
                kbps_since_last_time = (bytes_received_since_last_time / duration_since_last_time).kbps()
            print(f"    packets_received={sample.packets_received}; packets_lost={sample.packets_lost}; loss_percent={loss_percent}%; jitter={sample.jitter}; kbps={kbps_since_last_time}")
            last_sample = sample

def print_video_stats(stats_by_ssrc: Dict[int, List[VideoReceiveStats]]):
    print(f"Video Receive Stats by SSRC:")
    for (ssrc, stats) in stats_by_ssrc.items():
        print(f" from SSRC {ssrc}:")
        last_sample = None
        for sample in stats:
            loss_percent = "?"
            if sample.packets_received > 0:
                loss_percent = int(sample.packets_lost*100.0/(sample.packets_received + sample.packets_lost))
            kbps_since_last_time = "?"
            if last_sample:
                bytes_received_since_last_time = DataSize.from_bytes(sample.bytes_received - last_sample.bytes_received)
                duration_since_last_time = sample.time - last_sample.time
                kbps_since_last_time = (bytes_received_since_last_time / duration_since_last_time).kbps()
            print(f"    packets_received={sample.packets_received}; packets_lost={sample.packets_lost}; loss_percent={loss_percent}%; size={sample.frame_width}x{sample.frame_height}; kbps={kbps_since_last_time}")
            last_sample = sample

for path in sys.argv[1:]:
    file = open(path, 'r')
    start: Optional[CallStart] = None
    period: Optional[NetworkRoutePeriod] = None
    audio_receive_stats_by_ssrc: Dict[int, List[AudioReceiveStats]] = {}
    video_receive_stats_by_ssrc: Dict[int, List[VideoReceiveStats]] = {}
    for event in parse_events(file.readlines()):
        if isinstance(event, OutgoingCall):
            print(f"{event.time} Outgoing call")
            period = None

        if isinstance(event, IncomingCall):
            print(f"{event.time} Incoming call")
            period = None

        if isinstance(event, CallStart):
            print(f"{event.time} Call started (proceed); call_id = {event.call_id}")
            start = event
            period = None

        if isinstance(event, CallState):
            # These states aren't so interesting
            if event.state not in ["Terminating", "Terminated"]:
                print(f"{event.time} Call state: {event.state}")

        if isinstance(event, Connection):
            if event.remote_device_id == "0":
                print(f"{event.time} Create parent forking connection")
            else:
                print(f"{event.time} Create connection to remote device {event.remote_device_id}")

        if isinstance(event, LocalIceCandidate):
            print(f"{event.time} Local ICE candidate: {event.type}")

        if isinstance(event, RemoteIceCandidate):
            print(f"{event.time} Remote ICE candidate: {event.type}")

        if isinstance(event, IceConnected):
            print(f"{event.time} ICE connected")

        if isinstance(event, NetworkRouteChange):
            if period is not None and event.adapter_type != period.adapter_type:
                print(period.pretty(event.time))
                period = None
            if period is None:
                period = NetworkRoutePeriod(event.adapter_type, event.time, [])

        if isinstance(event, SendRateChange):
            if period is not None:
                period.send_rates.append(event.send_rate)

        if isinstance(event, LocalHangup):
            print(f"{event.time} Local Hangup")

        if isinstance(event, ReceivedHangup):
            print(f"{event.time} Received Hangup ({event.type}) from {event.remote_device_id}")

        if isinstance(event, AudioReceiveStats):
            audio_receive_stats_by_ssrc.setdefault(event.ssrc, []).append(event)

        if isinstance(event, VideoReceiveStats):
            video_receive_stats_by_ssrc.setdefault(event.ssrc, []).append(event)

        if isinstance(event, CallEnded):
            if period is not None:
                print(period.pretty(event.time))
            print(f"{event.time} Call ended")
            if start is not None:
                duration = event.time - start.time
                print(f"Duration: {duration}")
            print_stats(audio_receive_stats_by_ssrc, video_receive_stats_by_ssrc)
            audio_receive_stats_by_ssrc = {}
            video_receive_stats_by_ssrc = {}
            period = None

    # Just in case the call didn't end in the log
    print_stats(audio_receive_stats_by_ssrc, video_receive_stats_by_ssrc)


