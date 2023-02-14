#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

import gzip
import pandas as pd
import re
import requests
import zipfile

from io import BytesIO, StringIO
from pandas.api.types import is_numeric_dtype
from typing import Any, Optional

pd.set_option('display.precision', 1)
# Disable scientific notation (doesn't go well with SSRCs)
pd.set_option('display.float_format', lambda x: '%.1f' % x)

GROUP_CALL_TYPE = 'Group'

class Call():
    def __init__(self, logs: list[str], **kwargs: pd.DataFrame):
        self.connection = kwargs['connection']
        self.audio_send = kwargs['audio_send']
        self.audio_recv = kwargs['audio_recv']
        self.video_send = kwargs['video_send']
        self.video_recv = kwargs['video_recv']
        self.sfu_recv = kwargs['sfu_recv']
        self.ice_network_route_change = kwargs['ice_network_route_change']
        self._logs = logs
        self.start = kwargs['start']
        self.end = kwargs['end']
        self.type = kwargs['type']

    def ssrc(self) -> None:
        """
        Prints the SSRCs of the audio and the lowest layer video stream for
        the device the logs are from.
        """
        print(f'Audio {self.audio_send.ssrc[0]}')
        print(f'Video {self.video_send.ssrc[0]}')

    def describe_connection(self) -> None:
        self.connection[
            ['current_round_trip_time', 'available_outgoing_bitrate']
        ].plot(subplots=True, figsize=(10,10), grid=True)

    def describe_audio_send(self) -> None:
        self.audio_send[
            ['audio_energy', 'bitrate', 'remote_packets_lost_pct', 'remote_jitter', 'remote_round_trip_time']
        ].plot(subplots=True, figsize=(10,10), grid=True)

    def describe_audio_recv(self, ssrc: Optional[int] = None) -> None:
        if self.type == GROUP_CALL_TYPE:
            assert ssrc is not None, "SSRC required for group calls"

        df = self.audio_recv
        if ssrc is not None:
            df = self.audio_recv[self.audio_recv.ssrc == ssrc]

        df[
            ['audio_energy', 'bitrate', 'packets_lost_pct', 'jitter']
        ].plot(subplots=True, figsize=(10,10), grid=True)

    def describe_video_send(self, layer: Optional[int] = None) -> None:
        if layer is None and self.type == GROUP_CALL_TYPE:
            for i in range(0, 3):
                self._describe_video_send(i, f'Layer {i}')
        else:
            self._describe_video_send(layer if layer is not None else 0)

    def _describe_video_send(self, layer: int, title: Optional[str] = None) -> None:
        assert layer >= 0 and layer <= 2

        base_ssrc = self.video_send.ssrc[0]
        ssrc = base_ssrc + (layer * 2)

        ax = self.video_send[self.video_send.ssrc == ssrc][
            ['height', 'framerate', 'bitrate', 'key_frames_encoded', 'remote_packets_lost_pct', 'remote_jitter', 'remote_round_trip_time', 'retransmitted_bitrate', 'nack_count', 'pli_count']
        ].plot(subplots=True, figsize=(10,15), grid=True, title=title)[0]

        if title is not None:
            fig = ax.get_figure()
            # Remove extra whitespace between the title and the first plot
            fig.tight_layout()
            # And prevent the title from overlapping the plots
            fig.subplots_adjust(top=0.96)

    def describe_video_recv(self, ssrc: Optional[int] = None) -> None:
        if self.type == GROUP_CALL_TYPE:
            assert ssrc is not None, "SSRC required for group calls"

        df = self.video_recv
        if ssrc is not None:
            df = self.video_recv[self.video_recv.ssrc == ssrc]

        df[
            ['height', 'framerate', 'bitrate', 'key_frames_decoded', 'packets_lost_pct']
        ].plot(subplots=True, figsize=(10,10), grid=True)

    def describe_sfu_recv(self) -> None:
        self.sfu_recv[
            ['target_send_rate', 'ideal_send_rate', 'allocated_send_rate']
        ].plot(subplots=True, figsize=(10,10), grid=True)

    def logs(self, query: str = '') -> None:
        """
        Prints logs that were emitted during this call which contain `query`
        (case-insensitive). This includes application-level logs.
        """
        matched = (line for line in self._logs if query.casefold() in line.casefold())
        print('\n'.join(matched))


def _extract_logs(url: str, response: requests.Response) -> list[str]:
    if '/ios/' in url:
        f = zipfile.ZipFile(BytesIO(response.content))
        # Look at only the main logs
        log_files = sorted((name for name in f.namelist() if 'NSE' not in name and 'shareextension' not in name))

        raw_logs = ""

        for log in log_files:
            raw_logs += f.open(log).read().decode('utf-8')
    else:
        raw_logs = gzip.decompress(response.content).decode('utf-8')

    log_lines = raw_logs.split('\n')

    if '/ios/' in url:
        return log_lines
    else:
        # Look at only the main log section
        # ============ LOGGER ============= for android
        # ========= Logs ========= for desktop
        logger_start = next(i for i, line in enumerate(log_lines) if line == '============ LOGGER =============' or line == '========= Logs =========')
        return log_lines[logger_start + 1:]


def _parse_calls(logs: list[str]) -> list[Call]:
    def new_raw_call() -> dict[str, Any]:
        return {
            'connection': [],
            'ice_network_route_change': [],
            'audio_send': [],
            'audio_recv': [],
            'video_send': [],
            'video_recv': [],
            'sfu_recv': [],
            'logs': [],
            'start': '',
            'end': '',
            'type': 'Unknown',
        }

    def extract_timestamp(line: str) -> str:
        # e.g. "2022-12-03 11:38:08.357 CST"
        android = re.findall('\d+-\d+-\d+ \d+:\d+:\d+\.\d+ \w+', line)
        if android:
            return android[0]

        # e.g. "2022-11-28T00:41:41.299Z"
        desktop = re.findall('\d+-\d+-\d+T\d+:\d+:\d+\.\d+Z', line)
        if desktop:
            return desktop[0]

        # e.g. 2022/12/05 18:59:18:773
        ios = re.findall('\d+/\d+/\d+ \d+:\d+:\d+\:\d+', line)
        if ios:
            return ios[0]

        return line

    raw_calls = []
    raw_call = {}

    def append(key: str, value: str) -> None:
        if raw_call:
            raw_call[key].append(value)

    for line in logs:
        if 'on_start_call' in line:
            if raw_call:
                # If the application crashed, there won't be logs for the call
                # ending.
                raw_calls.append(dict(raw_call))
                raw_call.clear()

            raw_call = new_raw_call()
            raw_call['start'] = extract_timestamp(line)

            typ = re.findall('direction: (\w+)', line)
            if typ:
                raw_call['type'] = typ[0]
        elif 'Group Client created with id' in line:
            if raw_call:
                # If the application crashed, there won't be logs for the call
                # ending.
                raw_calls.append(dict(raw_call))
                raw_call.clear()

            raw_call = new_raw_call()
            raw_call['start'] = extract_timestamp(line)
            raw_call['type'] = GROUP_CALL_TYPE
        elif ('terminate_call' in line or 'delete_group_call_client' in line) and raw_call:
            raw_call['end'] = extract_timestamp(line)
            raw_calls.append(dict(raw_call))
            raw_call.clear()
        elif 'ice_network_route_change' in line:
            append('ice_network_route_change', line)
        elif 'ringrtc_stats!,connection,' in line:
            append('connection', line)
        elif 'ringrtc_stats!,audio,send' in line:
            append('audio_send', line)
        elif 'ringrtc_stats!,audio,recv' in line:
            append('audio_recv', line)
        elif 'ringrtc_stats!,video,send' in line:
            append('video_send', line)
        elif 'ringrtc_stats!,video,recv' in line:
            append('video_recv', line)
        elif 'ringrtc_stats!,sfu,recv' in line:
            append('sfu_recv', line)
        elif raw_call:
            raw_call['logs'].append(line)

    if raw_call:
        raw_calls.append(raw_call)

    def ice_network_route_change_lines_to_df(lines: list[str]) -> pd.DataFrame:
        # Generate a comma delimited representation of the ice network route change data
        csv = []

        csv.append("timestamp,timestamp_us,local_adapter_type,local_adapter_type_under_vpn,local_relayed,local_relay_protocol,remote_relayed")

        for line in lines:
            # Example Line:
            # INFO  2022-12-15T22:44:08.716Z src/webrtc/peer_connection_observer.rs:297 ringrtc!	1671144248716	rtc -> conn: ice_network_route_change(NetworkRoute { local_adapter_type: Unknown, local_adapter_type_under_vpn: Unknown, local_relayed: false, local_relay_protocol: Unknown, remote_relayed: false })	2

            # Parse out the logger timestamp
            log_timestamp = extract_timestamp(line)

            # Parse out the network route timestamp
            timestamp = line.split('\t')[1]

            if '{' not in line:
                continue

            # Parse out the NetworkRoute info
            line = line.split("{ ")[1]
            line = line.split(" }")[0]
            network_route_dict = {i.split(': ')[0]: i.split(': ')[1] for i in line.split(', ')}

            network_route_values = ','.join(map(str, network_route_dict.values()))

            csv.append(log_timestamp + "," + timestamp + "," + network_route_values)

        df = pd.read_csv(StringIO('\n'.join(csv)))

        # Convert bool to int(0/1) value to make it easier to plot in graphs
        df["local_relayed"] = df["local_relayed"].astype(int)
        df["remote_relayed"] = df["remote_relayed"].astype(int)

        return df

    def lines_to_df(lines: str) -> pd.DataFrame:
        if not lines:
            # Handle the case when there are no lines
            return pd.DataFrame()
        elif 'ringrtc_stats!,sfu,recv' in lines[0] and lines[0][-1].isdigit():
            # Older logs didn't include a header for SFU receive stats, so set
            # the column names manually.
            return pd.read_csv(StringIO('\n'.join(lines)),
                               names=['timestamp',
                                      'sfu',
                                      'recv',
                                      'target_send_rate',
                                      'ideal_send_rate',
                                      'allocated_send_rate'])
        else:
            return pd.read_csv(StringIO('\n'.join(lines)))

    def clean_columns(df: pd.DataFrame) -> pd.DataFrame:
        if df.columns.empty:
            # Outgoing calls with no answer have no stats headers.
            return df

        # Clean first column
        df = df.rename(columns={df.columns[0]: 'timestamp'})

        df['timestamp'] = df['timestamp'].transform(extract_timestamp)

        numeric_columns = [
            ('bitrate', 'bps'),
            ('available_outgoing_bitrate', 'bps'),
            ('retransmitted_bitrate', 'bps'),
            ('packets_lost_pct', '%'),
            ('remote_packets_lost_pct', '%'),
            ('jitter', 'ms'),
            ('remote_jitter', 'ms'),
            ('current_round_trip_time', 'ms'),
            ('remote_round_trip_time', 'ms'),
            ('encode_time_per_frame', 'ms'),
            ('decode_time_per_frame', 'ms'),
            ('send_delay_per_packet', 'ms'),
            ('framerate', 'fps'),
            ('packets_per_second', ''),
            ('average_packet_size', ''),
            ('audio_energy', ''),
            ('key_frames_encoded', ''),
            ('key_frames_decoded', ''),
            ('retransmitted_packets_sent', ''),
            ('nack_count', ''),
            ('pli_count', ''),
            ('quality_limitation_resolution_changes', ''),
            ('target_send_rate', ''),
            ('ideal_send_rate', ''),
            ('allocated_send_rate', ''),
        ]

        for (name, suffix) in numeric_columns:
            if name in df:
                while True:
                    if is_numeric_dtype(df[name]):
                        # If there's no duplicate stats header, and there are
                        # values for a column that doesn't have a suffix,
                        # read_csv will automatically convert it to a numeric
                        # type.
                        break

                    try:
                        df[name] = pd.to_numeric(df[name].transform(lambda s: s.rstrip(suffix)))
                    except ValueError:
                        # The stats header may appear multiple times for a call.
                        # Remove duplicate header entries
                        df = df.iloc[1: , :]
                    else:
                        break

        # Create individual width and height columns based on resolution
        if 'resolution' in df:
            # resolution is in the form: `640x480`
            df['width'] = pd.to_numeric(df.resolution.transform(lambda r: r.split('x')[0]))
            df['height'] = pd.to_numeric(df.resolution.transform(lambda r: r.split('x')[1]))

        return df

    def create_call(raw_call: dict[str, Any]) -> Call:
        return Call(
            connection=clean_columns(lines_to_df(raw_call['connection'])),
            audio_send=clean_columns(lines_to_df(raw_call['audio_send'])),
            audio_recv=clean_columns(lines_to_df(raw_call['audio_recv'])),
            video_send=clean_columns(lines_to_df(raw_call['video_send'])),
            video_recv=clean_columns(lines_to_df(raw_call['video_recv'])),
            sfu_recv=clean_columns(lines_to_df(raw_call['sfu_recv'])),
            ice_network_route_change=ice_network_route_change_lines_to_df(raw_call['ice_network_route_change']),
            logs=raw_call['logs'],
            start=raw_call['start'],
            end=raw_call['end'],
            type=raw_call['type'],
        )

    return [create_call(raw_call) for raw_call in raw_calls]


def load_calls(url: str) -> list[Call]:
    response = requests.get(url)
    logs = _extract_logs(url, response)
    return _parse_calls(logs)

def load_calls_from_file(path_to_file: str) -> list[Call]:
    with open(path_to_file, "r") as file:
        logs = file.read()
    return _parse_calls(logs.split('\n'))

def describe(calls: list[Call]) -> pd.DataFrame:
    def ssrc_count(call: Call) -> Optional[int]:
        if 'ssrc' not in call.audio_recv:
            # The first call may not have columns set if the call started
            # before the first line in the logs.
            return None

        return call.audio_recv.ssrc.unique().size if not call.audio_recv.empty else 0

    rows = [[call.type, call.start, call.end, ssrc_count(call)] for call in calls]
    return pd.DataFrame(rows, columns=['type', 'start', 'end', 'other_participants'])
