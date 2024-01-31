#
# Copyright 2024 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

FROM python:3.11

RUN python -m pip install scipy pesq

COPY pesq_mos.py .

ENTRYPOINT ["python", "./pesq_mos.py"]
