//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import * as os from 'os';
import * as process from 'process';

// eslint-disable-next-line @typescript-eslint/no-var-requires, import/no-dynamic-require
export default require(`../../build/${os.platform()}/libringrtc-${
  process.arch
}.node`);
