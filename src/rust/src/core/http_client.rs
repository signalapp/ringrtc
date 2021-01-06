//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! HTTP client backed by the native HTTP API of the platform.

use crate::common::{HttpMethod, HttpResponse};
use std::collections::HashMap;

pub trait HttpClient {
    fn make_request(
        &self,
        url: String,
        method: HttpMethod,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
        on_response: Box<dyn FnOnce(Option<HttpResponse>) + Send>,
    );
}
