#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# Based on https://github.com/twilio/twilio-video.js/blob/master/lib/preflight/mos.ts
def compute_emos(rtt: int, jitter: int, fractionLost: float) -> float:
    """
    Computes the MOS value between [1, 4.5) based on rtt, jitter, and packet loss
    """

    r0: float = 94.768

    effectiveLatency: int = rtt + (jitter * 2) + 10

    rFactor: float = 0
    if effectiveLatency < 160:
        rFactor = r0 - (effectiveLatency / 40)
    elif effectiveLatency < 1000:
        rFactor = r0 - ((effectiveLatency - 120) / 10)

    # Adjust "rFactor" with the fraction of packets lost.
    if fractionLost <= (rFactor / 2.5):
        rFactor = max(rFactor - fractionLost * 2.5, 6.52)
    else:
        rFactor = 0

    # Compute MOS from "rFactor".
    mos: float = 1 + (0.035 * rFactor) + (0.000007 * rFactor) * (rFactor - 60) * (100 - rFactor)

    return mos
