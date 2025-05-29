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
    public expiration: Date,
    public epoch?: CallLinkEpoch
  ) {}
}

export enum CallLinkRestrictions {
  None,
  AdminApproval,
  Unknown,
}

export class CallLinkEpoch {
  /** @internal */
  value: number;

  /** @internal */
  constructor(value: number) {
    this.value = value;
  }

  /** @internal */
  asNumber(): number {
    return this.value;
  }

  static parse(str: string): CallLinkEpoch {
    return new CallLinkEpoch(Native.CallLinkEpoch_parse(str));
  }

  toString(): string {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-return
    return Native.CallLinkEpoch_toFormattedString(this.value);
  }

  get bytes(): Uint8Array {
    const value = this.value & 0xffffffff;
    const bytes = new Uint8Array(4);
    bytes[0] = value & 0x000000ff;
    bytes[1] = (value & 0x0000ff00) >> 8;
    bytes[2] = (value & 0x00ff0000) >> 16;
    bytes[3] = (value & 0xff000000) >> 24;
    return bytes;
  }

  static fromBytes(bytes: Uint8Array): CallLinkEpoch {
    const value =
      bytes[0] + bytes[1] * 0x100 + 0x10000 * (bytes[2] + bytes[3] * 0x100);
    return new CallLinkEpoch(value);
  }
}
