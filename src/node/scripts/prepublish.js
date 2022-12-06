//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

if (!process.env.npm_package_config_prebuildChecksum) {
  throw new Error('must set prebuildChecksum before publishing');
}
