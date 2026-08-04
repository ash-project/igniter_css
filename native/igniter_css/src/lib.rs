// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! CSS codemods for Igniter, built on Biome's lossless CSS CST.
//!
//! The architecture in one line: **parse losslessly, locate byte ranges, splice
//! text**. Nothing here ever reprints the tree, which is why comments,
//! indentation and property order outside an edit are preserved by construction
//! rather than by effort.
//!
//! * [`ctx`] -- the source, its parse, and the file's own formatting habits
//! * [`locate`] -- typed CST queries that return byte ranges
//! * [`trivia`] -- which comments a deleted node owns
//! * [`edit`] -- overlap-checked text splicing
//! * [`ops`] -- the codemods, all diff-minimal and idempotent
//! * [`analyze`] -- read-only queries
//! * [`transform`] -- whole-file minify/beautify/merge, explicitly *not* codemods
//! * [`nif`] -- the Elixir boundary

pub mod analyze;
pub mod atoms;
pub mod ctx;
pub mod edit;
pub mod error;
pub mod helpers;
pub mod locate;
pub mod nif;
pub mod ops;
pub mod transform;
pub mod trivia;
