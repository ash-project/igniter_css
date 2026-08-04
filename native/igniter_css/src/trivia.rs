// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Comment ownership on delete.
//!
//! When a codemod removes a node, which of the comments around it go with it?
//! The convention, decided deliberately rather than inferred:
//!
//! | Comment position                                   | Fate      |
//! |----------------------------------------------------|-----------|
//! | trailing on the same line as the node              | deleted   |
//! | own line directly above, no blank line between     | deleted   |
//! | separated from the node by a blank line            | **kept**  |
//! | looks like a section header                        | **kept**  |
//!
//! A section header is a comment that spans more than one line, or that
//! contains a rule of three or more repeated `= - * # ~ _` characters. Those
//! read as headings for everything below them, not as documentation of the one
//! node that happens to follow.

use crate::ctx::ParseCtx;

/// A byte range to delete, widened from a node's own range to include the
/// comments and line terminator it owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteSpan {
    pub start: usize,
    pub end: usize,
}

/// True when a comment reads as a heading for the region below it rather than
/// as documentation of the single node that follows.
pub fn is_section_header(text: &str) -> bool {
    if text.contains('\n') {
        return true;
    }
    let mut run_char = '\0';
    let mut run = 0usize;
    for c in text.chars() {
        if c == run_char {
            run += 1;
            if run >= 3 && matches!(c, '=' | '-' | '*' | '#' | '~' | '_') {
                return true;
            }
        } else {
            run_char = c;
            run = 1;
        }
    }
    false
}

/// Comments in the file as `(start, end)` byte ranges, sorted. Computed once
/// per operation and threaded through, so a codemod touching many nodes does
/// not re-walk the tree for each one.
pub fn comment_ranges(ctx: &ParseCtx) -> Vec<(usize, usize)> {
    crate::locate::all_comments(ctx)
        .into_iter()
        .map(|(s, e, _)| (s, e))
        .collect()
}

fn is_blank(s: &str) -> bool {
    s.chars().all(|c| c == ' ' || c == '\t' || c == '\r')
}

/// The comment that starts exactly at `offset` after optional spaces/tabs.
fn comment_starting_at(comments: &[(usize, usize)], offset: usize) -> Option<(usize, usize)> {
    comments.iter().copied().find(|(s, _)| *s == offset)
}

/// The comment whose text ends inside `[line_start, line_end)`.
fn comment_ending_in_line(
    comments: &[(usize, usize)],
    line_start: usize,
    line_end: usize,
) -> Option<(usize, usize)> {
    comments
        .iter()
        .copied()
        .find(|(_, e)| *e > line_start && *e <= line_end)
}

/// Widen `[start, end)` to the bytes the node owns under Rule D.
///
/// `start`/`end` must be the node's *trimmed* range -- its own text, without
/// surrounding trivia.
pub fn deletion_span(
    ctx: &ParseCtx,
    comments: &[(usize, usize)],
    start: usize,
    end: usize,
) -> DeleteSpan {
    let src = ctx.source();
    let owns_its_line = ctx.is_at_line_start(start);

    // ---- forward: trailing comments on the same line, then the terminator.
    let mut e = end;
    loop {
        let mut probe = e;
        while matches!(src.as_bytes().get(probe), Some(b' ') | Some(b'\t')) {
            probe += 1;
        }
        match comment_starting_at(comments, probe) {
            // Only same-line: a comment on the next line is not ours.
            Some((_, c_end)) if !src[e..probe].contains('\n') => e = c_end,
            _ => break,
        }
    }
    if owns_its_line && ctx.is_at_line_end(e) {
        e = ctx.line_end_inclusive(e);
    }

    // ---- backward: own-line comments directly above.
    let mut s = start;
    if owns_its_line {
        s = ctx.line_start(start);
        while s > 0 {
            let prev_line_start = ctx.line_start(s - 1);
            let prev_line = &src[prev_line_start..s];
            if is_blank(prev_line.trim_end_matches('\n')) {
                // A blank line separates the comment from the node: keep it.
                break;
            }
            let Some((c_start, c_end)) = comment_ending_in_line(comments, prev_line_start, s)
            else {
                break;
            };
            // The comment must be alone on its line(s).
            if !is_blank(src[c_end..s].trim_end_matches('\n')) {
                break;
            }
            let c_line_start = ctx.line_start(c_start);
            if !is_blank(&src[c_line_start..c_start]) {
                break;
            }
            if is_section_header(&src[c_start..c_end]) {
                break;
            }
            s = c_line_start;
        }
    }

    DeleteSpan { start: s, end: e }
}

/// Collapse a run of blank lines left behind by a deletion down to one.
///
/// Deleting `.b` from `.a{}\n\n.b{}\n\n.c{}\n` would otherwise leave two blank
/// lines where the user had one. Returns a widened span, never a narrower one.
pub fn absorb_surrounding_blank_line(ctx: &ParseCtx, span: DeleteSpan) -> DeleteSpan {
    let src = ctx.source();
    let mut end = span.end;

    // Only relevant when the deletion consumed whole lines.
    if !(span.start == ctx.line_start(span.start)
        && (end == src.len() || src[..end].ends_with('\n')))
    {
        return span;
    }

    let before_is_blank = span.start == 0 || {
        let prev_start = ctx.line_start(span.start - 1);
        is_blank(src[prev_start..span.start].trim_end_matches('\n'))
    };

    if before_is_blank {
        // Drop one following blank line so we don't stack two.
        let next_end = ctx.line_end_inclusive(end);
        if next_end > end && is_blank(src[end..next_end].trim_end_matches('\n')) {
            end = next_end;
        }
    }

    DeleteSpan {
        start: span.start,
        end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locate::{declarations_in, find_rule_by_selector};

    fn span_for_decl(src: &str, selector: &str, property: &str) -> (usize, usize) {
        let ctx = ParseCtx::parse_default(src);
        let comments = comment_ranges(&ctx);
        let rule = find_rule_by_selector(&ctx, selector).one().unwrap();
        let d = declarations_in(&ctx, &rule)
            .into_iter()
            .find(|d| d.property == property)
            .unwrap();
        let s = deletion_span(&ctx, &comments, d.start, d.end);
        (s.start, s.end)
    }

    fn deleted_text(src: &str, selector: &str, property: &str) -> String {
        let (s, e) = span_for_decl(src, selector, property);
        src[s..e].to_string()
    }

    fn remaining(src: &str, selector: &str, property: &str) -> String {
        let (s, e) = span_for_decl(src, selector, property);
        format!("{}{}", &src[..s], &src[e..])
    }

    // -- section header detection -------------------------------------------

    #[test]
    fn a_plain_comment_is_not_a_section_header() {
        assert!(!is_section_header("/* Firefox */"));
        assert!(!is_section_header("/* the brand colour */"));
        assert!(!is_section_header("// a line comment"));
    }

    #[test]
    fn a_ruled_comment_is_a_section_header() {
        assert!(is_section_header("/* ===== Layout ===== */"));
        assert!(is_section_header("/* --- utilities --- */"));
        assert!(is_section_header("/* ### Section ### */"));
        assert!(is_section_header("/* ~~~ */"));
        assert!(is_section_header("/* ___ */"));
    }

    #[test]
    fn a_multi_line_comment_is_a_section_header() {
        assert!(is_section_header("/* line one\n   line two */"));
    }

    #[test]
    fn two_repeated_characters_are_not_a_rule() {
        assert!(!is_section_header("/* a--b */"));
        assert!(!is_section_header("/* == */"));
    }

    // -- Rule D cases -------------------------------------------------------

    #[test]
    fn a_trailing_same_line_comment_is_deleted_with_the_declaration() {
        let src = ".a {\n  color: red; /* legacy */\n  margin: 0;\n}\n";
        assert_eq!(
            deleted_text(src, ".a", "color"),
            "  color: red; /* legacy */\n"
        );
        assert_eq!(remaining(src, ".a", "color"), ".a {\n  margin: 0;\n}\n");
    }

    #[test]
    fn an_adjacent_own_line_comment_above_is_deleted_with_the_declaration() {
        let src = ".a {\n  /* brand colour */\n  color: red;\n  margin: 0;\n}\n";
        assert_eq!(
            deleted_text(src, ".a", "color"),
            "  /* brand colour */\n  color: red;\n"
        );
    }

    #[test]
    fn a_comment_separated_by_a_blank_line_is_kept() {
        let src = ".a {\n  /* used by the sidebar */\n\n  color: red;\n  margin: 0;\n}\n";
        assert_eq!(deleted_text(src, ".a", "color"), "  color: red;\n");
        assert_eq!(
            remaining(src, ".a", "color"),
            ".a {\n  /* used by the sidebar */\n\n  margin: 0;\n}\n"
        );
    }

    #[test]
    fn a_section_header_above_is_kept() {
        let src = ".a {\n  /* ===== Colours ===== */\n  color: red;\n  margin: 0;\n}\n";
        assert_eq!(deleted_text(src, ".a", "color"), "  color: red;\n");
    }

    #[test]
    fn a_multi_line_comment_above_is_kept() {
        let src = ".a {\n  /* why this exists:\n     because reasons */\n  color: red;\n}\n";
        assert_eq!(deleted_text(src, ".a", "color"), "  color: red;\n");
    }

    #[test]
    fn several_adjacent_comment_lines_are_all_deleted() {
        let src = ".a {\n  /* one */\n  /* two */\n  color: red;\n  margin: 0;\n}\n";
        assert_eq!(
            deleted_text(src, ".a", "color"),
            "  /* one */\n  /* two */\n  color: red;\n"
        );
    }

    #[test]
    fn an_adjacent_run_stops_at_a_section_header() {
        let src = ".a {\n  /* ==== Header ==== */\n  /* about colour */\n  color: red;\n}\n";
        assert_eq!(
            deleted_text(src, ".a", "color"),
            "  /* about colour */\n  color: red;\n"
        );
    }

    #[test]
    fn both_a_leading_and_a_trailing_comment_are_taken() {
        let src = ".a {\n  /* above */\n  color: red; /* beside */\n  margin: 0;\n}\n";
        assert_eq!(
            deleted_text(src, ".a", "color"),
            "  /* above */\n  color: red; /* beside */\n"
        );
    }

    #[test]
    fn a_declaration_sharing_a_line_does_not_eat_the_line() {
        let src = ".a { color: red; margin: 0; }\n";
        assert_eq!(deleted_text(src, ".a", "color"), "color: red;");
    }

    #[test]
    fn a_comment_on_the_next_line_is_not_a_trailing_comment() {
        let src = ".a {\n  color: red;\n  /* about margin */\n  margin: 0;\n}\n";
        assert_eq!(deleted_text(src, ".a", "color"), "  color: red;\n");
    }

    #[test]
    fn a_line_comment_above_is_deleted_with_the_declaration() {
        let src = ".a {\n  // brand colour\n  color: red;\n  margin: 0;\n}\n";
        assert_eq!(
            deleted_text(src, ".a", "color"),
            "  // brand colour\n  color: red;\n"
        );
    }

    #[test]
    fn crlf_line_endings_are_consumed_whole() {
        let src = ".a {\r\n  color: red;\r\n  margin: 0;\r\n}\r\n";
        assert_eq!(deleted_text(src, ".a", "color"), "  color: red;\r\n");
        assert_eq!(
            remaining(src, ".a", "color"),
            ".a {\r\n  margin: 0;\r\n}\r\n"
        );
    }

    #[test]
    fn a_string_that_looks_like_a_comment_is_not_treated_as_one() {
        let src = ".a {\n  content: \"/* not a comment */\";\n  color: red;\n}\n";
        // Deleting `color` must not swallow the `content` line.
        assert_eq!(deleted_text(src, ".a", "color"), "  color: red;\n");
    }

    // -- blank line collapsing ---------------------------------------------

    #[test]
    fn a_doubled_blank_line_is_collapsed() {
        let src = ".a {}\n\n.b {}\n\n.c {}\n";
        let ctx = ParseCtx::parse_default(src);
        let comments = comment_ranges(&ctx);
        let rule = find_rule_by_selector(&ctx, ".b").one().unwrap();
        let span = deletion_span(&ctx, &comments, rule.start, rule.end);
        let span = absorb_surrounding_blank_line(&ctx, span);
        let out = format!("{}{}", &src[..span.start], &src[span.end..]);
        assert_eq!(out, ".a {}\n\n.c {}\n");
    }

    #[test]
    fn a_single_blank_line_is_left_alone() {
        let src = ".a {}\n.b {}\n.c {}\n";
        let ctx = ParseCtx::parse_default(src);
        let comments = comment_ranges(&ctx);
        let rule = find_rule_by_selector(&ctx, ".b").one().unwrap();
        let span = deletion_span(&ctx, &comments, rule.start, rule.end);
        let span = absorb_surrounding_blank_line(&ctx, span);
        let out = format!("{}{}", &src[..span.start], &src[span.end..]);
        assert_eq!(out, ".a {}\n.c {}\n");
    }
}
