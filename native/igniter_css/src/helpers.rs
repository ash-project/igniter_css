// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Encoding helpers for the NIF boundary, matching the `{status, source,
//! payload}` shape `igniter_js` uses so both libraries feel the same from
//! Elixir.

use rustler::{Encoder, Env, NifResult, Term};

pub fn encode_response<T>(
    env: Env<'_>,
    status: rustler::types::atom::Atom,
    source: rustler::types::atom::Atom,
    message: T,
) -> NifResult<Term<'_>>
where
    T: Encoder,
{
    Ok((status, source, message).encode(env))
}
