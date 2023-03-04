//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

/* eslint-disable no-console */

import { chunk } from 'lodash';
import { assert } from 'chai';

export function countDownLatch(count: number): {
  countDown: () => void;
  finished: Promise<void>;
} {
  assert(count > 0, 'count must be a positive number');
  let resolve: () => void;
  const finished = new Promise<void>(resolveInternal => {
    resolve = resolveInternal;
  });

  const countDown = () => {
    count--;
    if (count == 0) {
      resolve();
    }
  };

  return {
    countDown: countDown,
    finished,
  };
}

export function log(line: string): void {
  // Standard logging used for checkpoints.
  // Use --renderer to see the log output. (edit: Maybe always shown now?)
  // BgYellow
  console.log(`\x1b[43m${line}\x1b[0m`);
}

export function sleep(timeout: number): Promise<void> {
  return new Promise<void>(resolve => {
    setTimeout(() => {
      // BgBlue
      console.log(`\x1b[44msleeping ${timeout} ms\x1b[0m`);
      resolve();
    }, timeout);
  });
}

export function uuidToBytes(uuid: string): Uint8Array {
  if (uuid.length !== 36) {
    return new Uint8Array(0);
  }

  return Uint8Array.from(
    chunk(uuid.replace(/-/g, ''), 2).map(pair => parseInt(pair.join(''), 16))
  );
}
