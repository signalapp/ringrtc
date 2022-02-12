#!/usr/bin/env python3

#
# Copyright 2022 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# To run: 
#
#  python3 parse_log.py 1.log 2.log ...
#

from enum import Enum
import re
import sys
from typing import Union, Optional, Iterator, NamedTuple, TypeVar, Iterable, List

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
    (hours, mins, secs) = (float(x) for x in unparsed.split(":"))
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

class DataRate(NamedTuple):
    bps: int

    def __repr__(self):
        return f"{self.bps/1000:.0f}kbps"

def parse_bps(unparsed: str) -> DataRate:
    return DataRate(int(unparsed))

CALL_START_PATTERN = re.compile(".*(\d\d:\d\d:\d\d.\d+).*ringrtc_stats!,connection,timestamp_us,current_round_trip_time,available_outgoing_bitrate.*")

class CallStart(NamedTuple):
    time: Instant

def parse_call_start(line: str) -> Optional[CallStart]:
    match = CALL_START_PATTERN.match(line)
    if match:
        (timestamp, ) = match.groups()
        time = parse_timestamp(timestamp)
        return CallStart(time)
    else:
        return None

CALL_END_PATTERN = re.compile(".*(\d\d:\d\d:\d\d.\d+).*ringrtc::core::call: terminate_connections\(\).*")
 
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

NETWORK_ROUTE_CHANGE_PATTERN = re.compile(".*(\d\d:\d\d:\d\d.\d+).*rtc -> conn: ice_network_route_change\(NetworkRoute { local_adapter_type: ([A-Za-z]+).*")

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

SEND_RATE_CHANGE_PATTERN = re.compile(".*(\d\d:\d\d:\d\d.\d+).*ringrtc_stats!,connection,.*?,.*?,(\d+).*")

class SendRateChange(NamedTuple):
    time: Instant
    send_rate: DataRate

def parse_send_rate_change_line(line: str) -> Optional[SendRateChange]:
    match = SEND_RATE_CHANGE_PATTERN.match(line)
    if match:
        (timestamp, send_rate_bps) = match.groups()
        time = parse_timestamp(timestamp)
        send_rate = parse_bps(send_rate_bps)
        return SendRateChange(time, send_rate)
    else:
        return None

Event = Union[NetworkRouteChange, SendRateChange, CallStart, CallEnded]
def parse_events(lines: Iterable[str]) -> Iterator[Event]:
    for line in lines:
        for parse in [parse_network_route_change, parse_send_rate_change_line, parse_call_start, parse_call_end]:
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

for path in sys.argv[1:]:
    file = open(path, 'r')
    period: Optional[NetworkRoutePeriod] = None
    for event in parse_events(file.readlines()):
        if isinstance(event, CallStart):
            print(f"Call started")
            period = None

        if isinstance(event, CallEnded):
            if period is not None:
                print(period.pretty(event.time))
            print(f"Call ended")
            period = None

        if isinstance(event, NetworkRouteChange):
            if period is not None and event.adapter_type != period.adapter_type:
                print(period.pretty(event.time))
                period = None
            if period is None:
                period = NetworkRoutePeriod(event.adapter_type, event.time, [])
        if isinstance(event, SendRateChange):
            if period is not None:
                period.send_rates.append(event.send_rate)



