#
# Copyright 2024 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

import sys
from scipy.io import wavfile
from pesq import pesq

_, ref = wavfile.read(sys.argv[1])
rate, deg = wavfile.read(sys.argv[2])

print(pesq(rate, ref, deg, 'wb'))
