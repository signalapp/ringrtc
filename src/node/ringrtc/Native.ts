//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import * as os from 'node:os';
import * as process from 'node:process';

// oxlint-disable-next-line typescript/no-var-requires import/no-dynamic-require node/global-require
export default require(
  `../../build/${os.platform()}/libringrtc-${process.arch}.node`
);
