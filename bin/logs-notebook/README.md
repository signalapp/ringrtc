# logs-notebook

A [Jupyter](https://jupyter.org/) notebook for analyzing RingRTC stats in
debuglogs.

## How to use

After setting up the dependencies, run the following command in this directory
to start Jupyter in your browser:

```shell
jupyter lab
```

Then there are two main functions that the notebook provides:

```python
import call_log_parser

calls = call_log_parser.load_calls("https://debuglogs.org/platform/version/hash")

call_log_parser.describe(calls)
```

`load_calls` takes a URL and returns a list of `Call`s (one for each call in
the log).

`describe` takes a list of `Call`s  and summarizes information about them into
a [pandas `DataFrame`](https://pandas.pydata.org/pandas-docs/stable/reference/api/pandas.DataFrame.html).

`load_calls` can also load multiple logs at once, and the results will include
only the calls that appear in all given logs:

```python
(caller, callee) = call_log_parser.load_calls(
    "https://debuglogs.org/platform/version/hash",
    "https://debuglogs.org/platform/version/hash2",
)
```

A single `Call` has the following attributes:

- `connection`
- `audio_send`
- `audio_recv`
- `video_send`
- `video_recv`
- `sfu_recv`
- `ice_network_route_change`

These correspond to the `ringrtc_stats!` and `ringrtc!` lines in the logs.

A `Call` also has this attribute:

- `media_key_recv`

The associated values of all of these attributes are a `DataFrame` of parsed
values.

The following methods are also available on `Call`:

|Method                              |Description|
|------------------------------------|-----------|
|`ssrc`                              |Prints the SSRCs of the audio and the lowest layer video stream of the participant who submitted the logs.|
|`describe_connection`               |Plots the `connection` stats.|
|`describe_audio_send`               |Plots the `audio,send` stats.|
|`describe_audio_recv`               |Plots the `audio,recv` stats. For group calls, the SSRC of the desired stream needs to be passed.|
|`describe_video_send`               |Plots the `video,send` stats. All video layers are plotted by default for group calls. Pass the index of the layer to show only one.|
|`describe_video_recv`               |Plots the `video,recv` stats. For group calls, the SSRC of the desired stream needs to be passed.|
|`describe_sfu_recv`                 |Plots the `sfu,recv` stats. Only for group calls.|
|`describe_system`                   |Plots the `system` stats.|
|`describe_ice_network_route_change` |Plots the `ice_network_route_change` local and remote relay values.|
|`logs`                              |Prints the logs for the call that contain the passed query.|

## Dependencies

These Python packages need to be accessible from the Jupyter environment for
the logs to be fetched and analyzed.

- [pandas](https://pypi.org/project/pandas/)
- [matplotlib](https://pypi.org/project/matplotlib/)
- [requests](https://pypi.org/project/requests/)
