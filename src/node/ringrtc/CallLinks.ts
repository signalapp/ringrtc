//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import Native from './Native';

export class CallLinkRootKey {
  readonly bytes: Buffer;

  private constructor(bytes: Buffer) {
    this.bytes = bytes;
  }

  static parse(str: string): CallLinkRootKey {
    return new CallLinkRootKey(Native.CallLinkRootKey_parse(str));
  }

  static fromBytes(bytes: Buffer): CallLinkRootKey {
    Native.CallLinkRootKey_validate(bytes);
    return new CallLinkRootKey(bytes);
  }

  static generate(): CallLinkRootKey {
    return new CallLinkRootKey(Native.CallLinkRootKey_generate());
  }

  static generateAdminPassKey(): Buffer {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-return
    return Native.CallLinkRootKey_generateAdminPasskey();
  }

  deriveRoomId(): Buffer {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-return
    return Native.CallLinkRootKey_deriveRoomId(this.bytes);
  }

  toString(): string {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-return
    return Native.CallLinkRootKey_toFormattedString(this.bytes);
  }
}

export class CallLinkState {
  constructor(
    public name: string,
    public restrictions: CallLinkRestrictions,
    public revoked: boolean,
    public expiration: Date
  ) {}
}

export enum CallLinkRestrictions {
  None,
  AdminApproval,
  Unknown,
}
