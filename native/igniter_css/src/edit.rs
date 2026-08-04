// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! The edit engine.
//!
//! Every codemod in this crate produces `Vec<Edit>` -- byte ranges into the
//! *original* source plus replacement text -- and never reprints the tree.
//! Anything outside an edit range is untouched by definition, which is why
//! comment preservation here is a property that cannot fail rather than one we
//! have to keep verifying.

use crate::error::{CssError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Byte offset into the original source.
    pub start: usize,
    /// Exclusive byte offset into the original source.
    pub end: usize,
    pub replacement: String,
}

impl Edit {
    pub fn replace(start: usize, end: usize, replacement: impl Into<String>) -> Self {
        Self {
            start,
            end,
            replacement: replacement.into(),
        }
    }

    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Self {
            start: at,
            end: at,
            replacement: text.into(),
        }
    }

    pub fn delete(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            replacement: String::new(),
        }
    }

    /// A pure insertion touches no existing bytes.
    pub fn is_insertion(&self) -> bool {
        self.start == self.end
    }
}

/// Splice `edits` into `source`.
///
/// * Overlapping ranges are a hard error -- we never silently merge them.
/// * Edits are applied back-to-front so earlier offsets stay valid.
/// * `apply_edits(src, vec![])` returns `src` byte for byte (Phase 1 acceptance).
pub fn apply_edits(source: &str, mut edits: Vec<Edit>) -> Result<String> {
    if edits.is_empty() {
        return Ok(source.to_string());
    }

    let len = source.len();
    for e in &edits {
        if e.start > e.end
            || e.end > len
            || !source.is_char_boundary(e.start)
            || !source.is_char_boundary(e.end)
        {
            return Err(CssError::BadRange {
                start: e.start,
                end: e.end,
                len,
            });
        }
    }

    // Sort ascending first so overlap detection only has to compare neighbours.
    // Ties are broken by `end` so that a pure insertion at offset N sorts before
    // a replacement starting at N, which is the only way two edits may share a
    // boundary offset.
    edits.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));

    for pair in edits.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        // Touching ranges (a.end == b.start) are fine. Two insertions at the
        // exact same offset are ambiguous in ordering, so we reject them too.
        let overlaps =
            b.start < a.end || (a.is_insertion() && b.is_insertion() && a.start == b.start);
        if overlaps {
            return Err(CssError::OverlappingEdits {
                first: (a.start, a.end),
                second: (b.start, b.end),
            });
        }
    }

    let grown: usize = edits.iter().map(|e| e.replacement.len()).sum();
    let mut out = String::with_capacity(len + grown);
    let mut cursor = 0usize;
    for e in &edits {
        out.push_str(&source[cursor..e.start]);
        out.push_str(&e.replacement);
        cursor = e.end;
    }
    out.push_str(&source[cursor..]);
    Ok(out)
}

/// Drop edits that would not change anything, so a codemod can report
/// `changed: false` honestly.
pub fn prune_noop_edits(source: &str, edits: Vec<Edit>) -> Vec<Edit> {
    edits
        .into_iter()
        .filter(|e| {
            if e.start > source.len() || e.end > source.len() {
                return true; // let apply_edits report the real error
            }
            source.get(e.start..e.end) != Some(e.replacement.as_str())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_edit_list_returns_source_byte_for_byte() {
        let src = "/* hi */\n.a { color: red; }\n";
        assert_eq!(apply_edits(src, vec![]).unwrap(), src);
    }

    #[test]
    fn single_replacement() {
        let src = ".a { color: red; }";
        let out = apply_edits(src, vec![Edit::replace(12, 15, "blue")]).unwrap();
        assert_eq!(out, ".a { color: blue; }");
    }

    #[test]
    fn multiple_edits_apply_back_to_front() {
        let src = "abcdef";
        let out = apply_edits(
            src,
            vec![Edit::replace(0, 1, "X"), Edit::replace(4, 6, "YZ!")],
        )
        .unwrap();
        assert_eq!(out, "XbcdYZ!");
    }

    #[test]
    fn insertion_at_offset() {
        let src = "ac";
        let out = apply_edits(src, vec![Edit::insert(1, "b")]).unwrap();
        assert_eq!(out, "abc");
    }

    #[test]
    fn touching_ranges_are_allowed() {
        let src = "abcdef";
        let out = apply_edits(
            src,
            vec![Edit::replace(0, 3, "X"), Edit::replace(3, 6, "Y")],
        )
        .unwrap();
        assert_eq!(out, "XY");
    }

    #[test]
    fn overlapping_ranges_are_rejected() {
        let src = "abcdef";
        let err = apply_edits(
            src,
            vec![Edit::replace(0, 4, "X"), Edit::replace(2, 6, "Y")],
        )
        .unwrap_err();
        assert!(matches!(err, CssError::OverlappingEdits { .. }));
    }

    #[test]
    fn two_insertions_at_the_same_offset_are_rejected() {
        let err = apply_edits("ab", vec![Edit::insert(1, "X"), Edit::insert(1, "Y")]).unwrap_err();
        assert!(matches!(err, CssError::OverlappingEdits { .. }));
    }

    #[test]
    fn insertion_at_the_start_of_a_replacement_is_allowed() {
        let out = apply_edits("abc", vec![Edit::insert(1, "-"), Edit::replace(1, 2, "B")]).unwrap();
        assert_eq!(out, "a-Bc");
    }

    #[test]
    fn out_of_bounds_range_is_rejected() {
        let err = apply_edits("abc", vec![Edit::replace(0, 99, "X")]).unwrap_err();
        assert!(matches!(err, CssError::BadRange { .. }));
    }

    #[test]
    fn inverted_range_is_rejected() {
        let err = apply_edits("abcdef", vec![Edit::replace(4, 2, "X")]).unwrap_err();
        assert!(matches!(err, CssError::BadRange { .. }));
    }

    #[test]
    fn non_char_boundary_is_rejected() {
        // "é" is two bytes; offset 1 splits it.
        let err = apply_edits("é", vec![Edit::replace(0, 1, "e")]).unwrap_err();
        assert!(matches!(err, CssError::BadRange { .. }));
    }

    #[test]
    fn multibyte_content_is_spliced_correctly() {
        let src = ".a::after { content: \"日本語\"; }";
        let start = src.find('日').unwrap();
        let end = start + "日本語".len();
        let out = apply_edits(src, vec![Edit::replace(start, end, "中文")]).unwrap();
        assert_eq!(out, ".a::after { content: \"中文\"; }");
    }

    #[test]
    fn prune_drops_edits_that_write_back_identical_text() {
        let src = ".a { color: red; }";
        let pruned = prune_noop_edits(src, vec![Edit::replace(12, 15, "red")]);
        assert!(pruned.is_empty());
    }

    #[test]
    fn prune_keeps_real_edits() {
        let src = ".a { color: red; }";
        let pruned = prune_noop_edits(src, vec![Edit::replace(12, 15, "blue")]);
        assert_eq!(pruned.len(), 1);
    }
}
