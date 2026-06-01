//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import Native from './Native';

export class CallLinkRootKey {
  readonly bytes: Uint8Array<ArrayBuffer>;

  constructor(bytes: Uint8Array<ArrayBuffer>) {
    this.bytes = bytes;
  }

  static parse(str: string): CallLinkRootKey {
    return new CallLinkRootKey(Native.CallLinkRootKey_parse(str));
  }

  static fromBytes(bytes: Uint8Array<ArrayBuffer>): CallLinkRootKey {
    Native.CallLinkRootKey_validate(bytes);
    return new CallLinkRootKey(bytes);
  }

  static generate(): CallLinkRootKey {
    return new CallLinkRootKey(Native.CallLinkRootKey_generate());
  }

  static generateAdminPassKey(): Uint8Array<ArrayBuffer> {
    // oxlint-disable-next-line typescript/no-unsafe-return
    return Native.CallLinkRootKey_generateAdminPasskey();
  }

  deriveRoomId(): Uint8Array<ArrayBuffer> {
    // oxlint-disable-next-line typescript/no-unsafe-return
    return Native.CallLinkRootKey_deriveRoomId(this.bytes);
  }

  toString(): string {
    // oxlint-disable-next-line typescript/no-unsafe-return
    return Native.CallLinkRootKey_toFormattedString(this.bytes);
  }
}

export class CallLinkState {
  constructor(
    public name: string,
    public restrictions: CallLinkRestrictions,
    public revoked: boolean,
    public expiration: Date,
    public rootKey: CallLinkRootKey
  ) {}
}

export enum CallLinkRestrictions {
  None,
  AdminApproval,
  Unknown,
}
