//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import { chunk } from 'lodash';
import { assert } from 'chai';

export function countDownLatch(count: number) {
  assert(count > 0, 'count must be a positive number');
  let resolve: Function;
  const finished = new Promise(resolveInternal => {
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

export function log(line: string) {
  // Standard logging used for checkpoints.
  // Use --renderer to see the log output. (edit: Maybe always shown now?)
  // BgYellow
  console.log(`\x1b[43m${line}\x1b[0m`);
}

export let sleep = (timeout: number) => {
  return new Promise<void>(resolve => {
    setTimeout(() => {
      // BgBlue
      console.log(`\x1b[44msleeping ${timeout} ms\x1b[0m`);
      resolve();
    }, timeout);
  });
};

export function uuidToBytes(uuid: string): Uint8Array {
  if (uuid.length !== 36) {
    return new Uint8Array(0);
  }

  return Uint8Array.from(
    chunk(uuid.replace(/-/g, ''), 2).map(pair => parseInt(pair.join(''), 16))
  );
}
