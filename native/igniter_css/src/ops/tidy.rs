// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Whole-file tidying ops that are nevertheless **diff-minimal**.
//!
//! Sorting and de-duplication are usually implemented by reprinting the tree,
//! which violates hard constraint #2. Here they are implemented as permutations
//! and deletions of existing byte ranges instead: lines move or disappear, and
//! every other byte in the file is untouched. A block we cannot rearrange
//! safely is skipped and reported in `diagnostics` rather than reformatted.

use crate::ctx::{ParseCtx, ParseOptions};
use crate::edit::Edit;
use crate::error::Result;
use crate::locate::{declaration_lists, find_top_level_rules, normalize_property, DeclRef};
use crate::ops::{run, Outcome};
use crate::trivia::{absorb_surrounding_blank_line, comment_ranges, deletion_span};

/// The byte range a declaration owns, including the comments attached to it.
fn owned_span(ctx: &ParseCtx, comments: &[(usize, usize)], d: &DeclRef) -> (usize, usize) {
    let s = deletion_span(ctx, comments, d.start, d.end);
    (s.start, s.end)
}

/// A block can be rearranged only when every declaration owns a contiguous run
/// of whole lines and nothing but blank space sits between them. Anything else
/// -- a section-header comment between two declarations, a nested rule, two
/// declarations sharing a line -- means a permutation could lose or misplace
/// text, so we decline.
fn spans_are_permutable(ctx: &ParseCtx, spans: &[(usize, usize)]) -> bool {
    if spans.len() < 2 {
        return false;
    }
    let src = ctx.source();
    for (i, (s, e)) in spans.iter().enumerate() {
        if *s != ctx.line_start(*s) || !src[..*e].ends_with('\n') {
            return false;
        }
        if i > 0 {
            let prev_end = spans[i - 1].1;
            if prev_end > *s {
                return false;
            }
            if !src[prev_end..*s].trim().is_empty() {
                return false;
            }
        }
    }
    true
}

/// Sort declarations alphabetically within each block.
///
/// Note this is a *semantic* change when a block mixes shorthand and longhand
/// (`margin` then `margin-left` behaves differently from the reverse). The sort
/// is stable, so repeated declarations of one property keep their relative
/// order and the last-wins rule is preserved.
pub fn sort_properties(source: &str, options: ParseOptions) -> Result<Outcome> {
    let mut skipped = 0usize;
    let outcome = run(source, options, |ctx| {
        let comments = comment_ranges(ctx);
        let mut edits = Vec::new();

        for (list, decls) in declaration_lists(ctx) {
            // Any non-declaration sibling (a nested rule, an `@apply`) means the
            // ordering carries meaning we must not disturb.
            if list.children().count() != decls.len() || decls.len() < 2 {
                continue;
            }
            let spans: Vec<(usize, usize)> = decls
                .iter()
                .map(|d| owned_span(ctx, &comments, d))
                .collect();
            if !spans_are_permutable(ctx, &spans) {
                skipped += 1;
                continue;
            }

            let mut order: Vec<usize> = (0..decls.len()).collect();
            order.sort_by(|a, b| {
                normalize_property(&decls[*a].property)
                    .cmp(&normalize_property(&decls[*b].property))
            });
            if order.iter().enumerate().all(|(i, j)| i == *j) {
                continue;
            }

            let start = spans[0].0;
            let end = spans[spans.len() - 1].1;
            let text: String = order
                .iter()
                .map(|i| {
                    let (s, e) = spans[*i];
                    &ctx.source()[s..e]
                })
                .collect();
            edits.push(Edit::replace(start, end, text));
        }
        Ok(edits)
    })?;

    Ok(if skipped > 0 {
        outcome.with_diagnostic(format!(
            "{skipped} block(s) left unsorted: their declarations are not on separate lines, \
             or a comment between them made the order meaningful"
        ))
    } else {
        outcome
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DedupeOptions {
    /// Drop an earlier declaration when a later one in the same block sets the
    /// same property.
    pub declarations: bool,
    /// Drop an earlier top-level rule when a later one has the same selector
    /// and an identical body.
    pub rules: bool,
}

impl Default for DedupeOptions {
    fn default() -> Self {
        Self {
            declarations: true,
            rules: true,
        }
    }
}

/// Remove redundant declarations and rules.
///
/// Only removals that cannot change rendering are made:
///
/// * a declaration is dropped only when a **later** declaration in the same
///   block sets the same property and is at least as important -- CSS's
///   last-wins rule already made the earlier one dead;
/// * a rule is dropped only when a later top-level rule has the same selector
///   **and** a byte-identical body.
pub fn remove_duplicates(
    source: &str,
    dedupe: DedupeOptions,
    options: ParseOptions,
) -> Result<Outcome> {
    run(source, options, |ctx| {
        let comments = comment_ranges(ctx);
        let mut edits: Vec<Edit> = Vec::new();

        if dedupe.declarations {
            for (_, decls) in declaration_lists(ctx) {
                for (i, d) in decls.iter().enumerate() {
                    let shadowed = decls[i + 1..].iter().any(|later| {
                        normalize_property(&later.property) == normalize_property(&d.property)
                            // An earlier `!important` beats a later plain one,
                            // so only a later flag of equal-or-greater weight
                            // makes this one dead.
                            && (later.important || !d.important)
                    });
                    if !shadowed {
                        continue;
                    }
                    let (s, e) = owned_span(ctx, &comments, d);
                    edits.push(Edit::delete(s, e));
                }
            }
        }

        if dedupe.rules {
            let rules = find_top_level_rules(ctx);
            for (i, r) in rules.iter().enumerate() {
                let body = ctx.source()[r.body_open..r.body_close].trim();
                let duplicated = rules[i + 1..].iter().any(|later| {
                    later.selector_norm == r.selector_norm
                        && ctx.source()[later.body_open..later.body_close].trim() == body
                });
                if !duplicated {
                    continue;
                }
                let span = deletion_span(ctx, &comments, r.start, r.end);
                let span = absorb_surrounding_blank_line(ctx, span);
                // A rule deletion subsumes any declaration deletions inside it.
                edits.retain(|e| !(e.start >= span.start && e.end <= span.end));
                edits.push(Edit::delete(span.start, span.end));
            }
        }

        Ok(edits)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> ParseOptions {
        ParseOptions::default()
    }

    fn sort(src: &str) -> Outcome {
        sort_properties(src, opts()).unwrap()
    }

    fn dedupe(src: &str) -> Outcome {
        remove_duplicates(src, DedupeOptions::default(), opts()).unwrap()
    }

    // -- sorting ------------------------------------------------------------

    #[test]
    fn sorts_declarations_alphabetically() {
        let src = ".a {\n  color: red;\n  background: blue;\n  z-index: 1;\n}\n";
        let o = sort(src);
        assert!(o.changed);
        assert_eq!(
            o.source,
            ".a {\n  background: blue;\n  color: red;\n  z-index: 1;\n}\n"
        );
    }

    #[test]
    fn sorting_moves_comments_with_their_declaration() {
        let src = ".a {\n  /* about z */\n  z-index: 1;\n  color: red; /* about c */\n}\n";
        let o = sort(src);
        assert_eq!(
            o.source,
            ".a {\n  color: red; /* about c */\n  /* about z */\n  z-index: 1;\n}\n"
        );
    }

    #[test]
    fn already_sorted_input_is_unchanged() {
        let src = ".a {\n  background: blue;\n  color: red;\n}\n";
        let o = sort(src);
        assert!(!o.changed);
        assert_eq!(o.source, src);
    }

    #[test]
    fn sorting_is_idempotent() {
        let src = ".a {\n  z-index: 1;\n  color: red;\n  background: blue;\n}\n";
        let once = sort(src);
        let twice = sort(&once.source);
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn sorting_leaves_everything_outside_the_block_alone() {
        let src = "/* head */\n.a {\n  b: 2;\n  a: 1;\n}\n/* tail */\n.z { q: 1; }\n";
        let o = sort(src);
        assert_eq!(
            o.source,
            "/* head */\n.a {\n  a: 1;\n  b: 2;\n}\n/* tail */\n.z { q: 1; }\n"
        );
    }

    #[test]
    fn a_single_line_block_is_skipped_and_reported() {
        let src = ".a { z-index: 1; color: red; }\n";
        let o = sort(src);
        assert!(!o.changed);
        assert_eq!(o.source, src);
        assert_eq!(o.diagnostics.len(), 1);
    }

    #[test]
    fn a_block_with_a_nested_rule_is_left_alone() {
        let src = ".a {\n  z-index: 1;\n  color: red;\n  &:hover { color: blue; }\n}\n";
        let o = sort(src);
        assert!(!o.changed);
    }

    #[test]
    fn a_block_with_an_apply_is_left_alone() {
        let src = ".a {\n  z-index: 1;\n  @apply px-2;\n  color: red;\n}\n";
        let o = sort(src);
        assert!(!o.changed);
    }

    #[test]
    fn a_section_header_between_declarations_blocks_sorting() {
        let src = ".a {\n  z-index: 1;\n\n  /* ===== colours ===== */\n  color: red;\n}\n";
        let o = sort(src);
        assert!(!o.changed);
        assert_eq!(o.diagnostics.len(), 1);
    }

    #[test]
    fn sorting_is_stable_for_a_repeated_property() {
        let src = ".a {\n  color: red;\n  color: blue;\n  background: x;\n}\n";
        let o = sort(src);
        assert_eq!(
            o.source,
            ".a {\n  background: x;\n  color: red;\n  color: blue;\n}\n"
        );
    }

    #[test]
    fn sorts_inside_media_blocks_too() {
        let src = "@media print {\n  .a {\n    z-index: 1;\n    color: red;\n  }\n}\n";
        let o = sort(src);
        assert_eq!(
            o.source,
            "@media print {\n  .a {\n    color: red;\n    z-index: 1;\n  }\n}\n"
        );
    }

    #[test]
    fn sorting_preserves_crlf() {
        let src = ".a {\r\n  z-index: 1;\r\n  color: red;\r\n}\r\n";
        let o = sort(src);
        assert_eq!(o.source, ".a {\r\n  color: red;\r\n  z-index: 1;\r\n}\r\n");
    }

    // -- de-duplication -----------------------------------------------------

    #[test]
    fn drops_a_shadowed_declaration() {
        let src = ".a {\n  color: red;\n  margin: 0;\n  color: blue;\n}\n";
        let o = dedupe(src);
        assert!(o.changed);
        assert_eq!(o.source, ".a {\n  margin: 0;\n  color: blue;\n}\n");
    }

    #[test]
    fn keeps_an_important_declaration_a_later_plain_one_cannot_override() {
        let src = ".a {\n  color: red !important;\n  color: blue;\n}\n";
        let o = dedupe(src);
        assert!(!o.changed);
        assert_eq!(o.source, src);
    }

    #[test]
    fn a_later_important_declaration_does_shadow_an_earlier_one() {
        let src = ".a {\n  color: red;\n  color: blue !important;\n}\n";
        let o = dedupe(src);
        assert_eq!(o.source, ".a {\n  color: blue !important;\n}\n");
    }

    #[test]
    fn drops_an_identical_duplicated_rule() {
        let src = ".a {\n  color: red;\n}\n\n.b {}\n\n.a {\n  color: red;\n}\n";
        let o = dedupe(src);
        assert_eq!(o.source, ".b {}\n\n.a {\n  color: red;\n}\n");
    }

    #[test]
    fn keeps_two_rules_with_the_same_selector_but_different_bodies() {
        let src = ".a { color: red; }\n.a { margin: 0; }\n";
        let o = dedupe(src);
        assert!(!o.changed);
    }

    #[test]
    fn does_not_touch_rules_in_different_scopes() {
        let src = ".a { color: red; }\n@media print {\n  .a { color: red; }\n}\n";
        let o = dedupe(src);
        assert!(!o.changed);
    }

    #[test]
    fn dedupe_is_idempotent() {
        let src =
            ".a {\n  color: red;\n  color: blue;\n}\n.a {\n  color: red;\n  color: blue;\n}\n";
        let once = dedupe(src);
        let twice = dedupe(&once.source);
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn deduping_takes_the_comment_owned_by_the_dropped_declaration() {
        let src = ".a {\n  /* old */\n  color: red;\n  color: blue;\n}\n";
        let o = dedupe(src);
        assert_eq!(o.source, ".a {\n  color: blue;\n}\n");
    }

    #[test]
    fn deduping_keeps_a_section_header() {
        let src = ".a {\n  /* ==== colours ==== */\n  color: red;\n  color: blue;\n}\n";
        let o = dedupe(src);
        assert_eq!(
            o.source,
            ".a {\n  /* ==== colours ==== */\n  color: blue;\n}\n"
        );
    }

    #[test]
    fn declaration_dedupe_can_be_switched_off() {
        let src = ".a {\n  color: red;\n  color: blue;\n}\n";
        let o = remove_duplicates(
            src,
            DedupeOptions {
                declarations: false,
                rules: true,
            },
            opts(),
        )
        .unwrap();
        assert!(!o.changed);
    }

    #[test]
    fn rule_dedupe_can_be_switched_off() {
        let src = ".a { color: red; }\n.a { color: red; }\n";
        let o = remove_duplicates(
            src,
            DedupeOptions {
                declarations: true,
                rules: false,
            },
            opts(),
        )
        .unwrap();
        assert!(!o.changed);
    }

    #[test]
    fn dropping_a_whole_rule_does_not_collide_with_dropping_its_declarations() {
        // `.a` is duplicated *and* has an internally shadowed declaration; the
        // rule deletion must subsume the declaration deletion, not overlap it.
        let src =
            ".a {\n  color: red;\n  color: blue;\n}\n.a {\n  color: red;\n  color: blue;\n}\n";
        let o = dedupe(src);
        assert_eq!(o.source, ".a {\n  color: blue;\n}\n");
    }
}
