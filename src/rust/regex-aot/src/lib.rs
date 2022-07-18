//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Uses regex-automata's serialization support to build regexes at compile time.
//!
//! ```
//! let re: regex_automata::Regex<_> = regex_aot::regex!(".+@.+");
//! ```

use quote::quote;

fn regex_impl(input: syn::LitStr) -> Result<proc_macro2::TokenStream, regex_automata::Error> {
    // Possible future work:
    // - figure out how big the state ID size needs to be in advance, like ucd-generate does.
    // - emit both little- and big-endian forms, guarded by cfg(...).
    // - accept several adjacent literals so the regex can be broken up into multiple lines.
    // - let the caller choose between a SparseDFA (smaller) and a DenseDFA (faster)
    let regex = regex_automata::Regex::new(&input.value())?;
    let fwd_bytes = regex
        .forward()
        .to_sparse_sized::<u16>()?
        .to_bytes_little_endian()?;
    let fwd_bytes = proc_macro2::Literal::byte_string(&fwd_bytes);
    let rev_bytes = regex
        .reverse()
        .to_sparse_sized::<u16>()?
        .to_bytes_little_endian()?;
    let rev_bytes = proc_macro2::Literal::byte_string(&rev_bytes);
    Ok(quote! { {
        #[cfg(not(target_endian = "little"))]
        compile_error!("only little-endian platforms are supported");
        let fwd: ::regex_automata::SparseDFA<&[u8], u16> =
            unsafe { ::regex_automata::SparseDFA::from_bytes(#fwd_bytes) };
        let rev: ::regex_automata::SparseDFA<&[u8], u16> =
            unsafe { ::regex_automata::SparseDFA::from_bytes(#rev_bytes) };
        ::regex_automata::Regex::from_dfas(fwd, rev)
    }})
}

#[proc_macro]
pub fn regex(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::LitStr);
    regex_impl(input)
        .unwrap_or_else(|e| {
            let msg = e.to_string();
            quote! {
                compile_error!(#msg)
            }
        })
        .into()
}
