//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

// TODO(mutexlox): Remove these after 2024 upgrade
#![warn(unsafe_attr_outside_unsafe)]
#![warn(unsafe_op_in_unsafe_fn)]
#![warn(missing_unsafe_on_extern)]
#![warn(rust_2024_incompatible_pat)]
#![warn(keyword_idents_2024)]

mod merge_buffer;
mod stream;
mod window;

pub use stream::{MrpHeader, MrpReceiveError, MrpSendError, MrpStream, PacketWrapper};
