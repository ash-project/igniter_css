// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

use std::fmt;

/// Every fallible path in this crate returns `CssError`. Nothing reachable from
/// a NIF call is allowed to panic -- a panic takes down the BEAM scheduler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CssError {
    /// Two edits produced by one operation cover overlapping byte ranges. This
    /// is always an internal bug; we hard-error rather than silently merging.
    OverlappingEdits {
        first: (usize, usize),
        second: (usize, usize),
    },
    /// An edit range fell outside the source, or did not land on a UTF-8
    /// character boundary.
    BadRange {
        start: usize,
        end: usize,
        len: usize,
    },
    /// More than one top-level rule matched the selector. ROADMAP §2 rule 4 and
    /// §11 R4: error, never guess.
    AmbiguousSelector { selector: String, count: usize },
    /// The caller asked to operate on something that isn't there.
    NotFound(String),
    /// The source could not be understood well enough to patch safely.
    Unparseable(String),
    /// Caller-supplied text (a selector, a raw block, an at-rule line) is not
    /// something we are willing to splice into a file.
    InvalidInput(String),
}

impl fmt::Display for CssError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OverlappingEdits { first, second } => write!(
                f,
                "internal error: overlapping edits {}..{} and {}..{}",
                first.0, first.1, second.0, second.1
            ),
            Self::BadRange { start, end, len } => write!(
                f,
                "internal error: edit range {start}..{end} is not a valid char boundary in a {len}-byte source"
            ),
            Self::AmbiguousSelector { selector, count } => write!(
                f,
                "selector {selector:?} matches {count} top-level rules; refusing to guess which one to patch"
            ),
            Self::NotFound(what) => write!(f, "not found: {what}"),
            Self::Unparseable(why) => write!(f, "cannot safely patch this file: {why}"),
            Self::InvalidInput(why) => write!(f, "invalid input: {why}"),
        }
    }
}

impl std::error::Error for CssError {}

pub type Result<T> = std::result::Result<T, CssError>;
