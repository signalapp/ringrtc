#
# Copyright 2024 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

FROM python:3.11

RUN mkdir repo \
    && cd repo \
    && git init . \
    && git remote add origin https://github.com/microsoft/PLC-Challenge.git \
    && git fetch --depth 1 origin c7a6ee438c1c42a7c668f3a19bf3ca0c6aad195d \
    && git checkout FETCH_HEAD

WORKDIR repo/PLCMOS

RUN python -m pip install -r requirements_standalone.txt

ENTRYPOINT ["python", "./plc_mos.py"]
