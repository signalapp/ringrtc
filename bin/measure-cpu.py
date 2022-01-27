#!/usr/bin/env python3

#
# Copyright 2019-2021 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# Before running:
#
#  pip install --no-binary :all psutil
#
#
# To run: 
#
#  python3 measure-cpu.py 5 electron
#
# or:
#
#  ./measure-cpu.py $duration_secs $process_name $iterations
#
# For more info: https://pypi.org/project/psutil/

import psutil
import sys
import time

def get_arg(index, parse, default):
    try:
        return parse(sys.argv[index])
    except:
        return default

def proc_matches_search(proc, search):
    if search is None:
        return True
    else:
        return search.lower() in proc.name().lower()

def get_name(proc):
    try:
        return proc.name()
    except:
        return "Unknown"

def get_cpu_percent(proc):
    try:
        return proc.cpu_percent()
    except:
        return 0

duration = get_arg(1, int, 1)
search = get_arg(2, str, None)
iterations = get_arg(3, int, 1)

print(f"Searching for processes that contain the name '{search}'", flush=True)

procs = [proc for proc in psutil.process_iter() if proc_matches_search(proc, search)]
names = set((get_name(proc) for proc in procs))

print(f"Getting CPU for processes with the names {names}", flush=True)

_ = [get_cpu_percent(proc) for proc in procs]

if iterations > 1:
    print(f"Waiting for {duration} seconds in {iterations} iterations", flush=True)
else:
    print(f"Waiting for {duration} seconds", flush=True)

samples = []
for i in range(iterations):
    time.sleep(duration)

    total_cpu_percent = sum((get_cpu_percent(proc) for proc in procs))
    samples.append(total_cpu_percent)

    print (f"Total CPU percent for iteration {i}: {total_cpu_percent:.2f}", flush=True)

if iterations > 1:
    average = sum(samples)/len(samples)
    print (f"Average across iterations: {average:.2f}", flush=True)
