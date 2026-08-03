// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Rule-level codemods and the shared "put a line inside this body" machinery
//! that the declaration ops build on.

use crate::ctx::{ParseCtx, ParseOptions};
use crate::edit::Edit;
use crate::error::{CssError, Result};
use crate::locate::{
    find_rule_by_selector, find_top_level_rules, normalize_selector, MatchResult, RuleRef,
};
use crate::ops::{
    past_trailing_comment, reindent, run, split_declarations, validate_snippet, Outcome,
};
use crate::trivia::{absorb_surrounding_blank_line, comment_ranges, deletion_span};
use biome_css_syntax::CssSyntaxKind;

/// Resolve a selector to exactly one top-level rule, or explain why not.
pub fn resolve_rule(ctx: &ParseCtx, selector: &str) -> Result<Option<RuleRef>> {
    match find_rule_by_selector(ctx, selector) {
        MatchResult::One(r) => Ok(Some(*r)),
        MatchResult::None => Ok(None),
        MatchResult::Ambiguous(hits) => Err(CssError::AmbiguousSelector {
            selector: selector.to_string(),
            count: hits.len(),
        }),
    }
}

/// Items directly inside a rule body, in source order: declarations, nested
/// rules and nested at-rules alike.
fn body_items(rule: &RuleRef) -> Vec<biome_css_syntax::CssSyntaxNode> {
    rule.node
        .children()
        .find(|c| {
            c.first_token()
                .is_some_and(|t| t.kind() == CssSyntaxKind::L_CURLY)
        })
        .into_iter()
        .flat_map(|block| block.children())
        .filter(|c| {
            matches!(
                c.kind(),
                CssSyntaxKind::CSS_DECLARATION_OR_RULE_LIST
                    | CssSyntaxKind::CSS_DECLARATION_LIST
                    | CssSyntaxKind::CSS_DECLARATION_OR_AT_RULE_LIST
                    | CssSyntaxKind::CSS_RULE_LIST
            )
        })
        .flat_map(|list| list.children())
        .collect()
}

/// Build the edits that append `text` (already `;`-terminated where relevant)
/// as the last item of `rule`'s body.
///
/// Honours rules B and C: the new line copies the indentation of the sibling it
/// lands next to, and a rule written entirely on one line stays on one line.
pub fn append_to_body(ctx: &ParseCtx, rule: &RuleRef, text: &str) -> Vec<Edit> {
    let nl = ctx.nl();
    let rule_indent = ctx.indent_at(rule.start).to_string();
    let single_line = !ctx.source()[rule.start..rule.end].contains('\n');
    let items = body_items(rule);

    if items.is_empty() {
        let replacement = if single_line {
            format!(" {text} ")
        } else {
            format!("{nl}{rule_indent}{}{text}{nl}{rule_indent}", ctx.indent())
        };
        return vec![Edit::replace(rule.body_open, rule.body_close, replacement)];
    }

    let comments = comment_ranges(ctx);
    let last = items.last().expect("non-empty");
    let last_start = usize::from(last.text_trimmed_range().start());
    let last_end = usize::from(last.text_trimmed_range().end());

    // A declaration that is not `;`-terminated needs one before we add a sibling.
    let is_declaration = matches!(
        last.kind(),
        CssSyntaxKind::CSS_DECLARATION | CssSyntaxKind::CSS_DECLARATION_WITH_SEMICOLON
    );
    let semi = if is_declaration && !ctx.source()[last_start..last_end].trim_end().ends_with(';') {
        ";"
    } else {
        ""
    };

    // Land after any comment trailing the last item, not between them.
    let after = past_trailing_comment(ctx, &comments, last_end);

    let indent = if ctx.is_at_line_start(last_start) {
        ctx.indent_at(last_start).to_string()
    } else {
        format!("{rule_indent}{}", ctx.indent())
    };
    let tail = if single_line {
        format!(" {text}")
    } else {
        format!("{nl}{indent}{text}")
    };

    if after == last_end {
        vec![Edit::insert(last_end, format!("{semi}{tail}"))]
    } else if semi.is_empty() {
        vec![Edit::insert(after, tail)]
    } else {
        vec![Edit::insert(last_end, semi), Edit::insert(after, tail)]
    }
}

/// Text of a brand new top-level rule, indented at column zero.
fn new_rule_text(ctx: &ParseCtx, selector: &str, body: &[String]) -> String {
    let nl = ctx.nl();
    if body.is_empty() {
        return format!("{selector} {{{nl}}}");
    }
    let inner = body
        .iter()
        .map(|d| format!("{}{d}", ctx.indent()))
        .collect::<Vec<_>>()
        .join(nl);
    format!("{selector} {{{nl}{inner}{nl}}}")
}

/// Append a new top-level rule at the end of the file.
pub fn append_rule_edits(ctx: &ParseCtx, selector: &str, body: &[String]) -> Vec<Edit> {
    let nl = ctx.nl();
    let src = ctx.source();
    let text = new_rule_text(ctx, selector, body);

    if src.trim().is_empty() {
        // Preserve whatever leading trivia (a header comment) already exists.
        let sep = if src.is_empty() || src.ends_with('\n') {
            ""
        } else {
            nl
        };
        return vec![Edit::insert(src.len(), format!("{sep}{text}{nl}"))];
    }

    // One blank line before the new rule, matching the file's own habit of
    // separating top-level rules.
    let trimmed_end = src.trim_end_matches(['\n', '\r', ' ', '\t']).len();
    let tail = if ctx.has_final_newline() {
        format!("{nl}{nl}{text}{nl}")
    } else {
        format!("{nl}{nl}{text}")
    };
    vec![Edit::replace(trimmed_end, src.len(), tail)]
}

// ---------------------------------------------------------------------------
// Public ops
// ---------------------------------------------------------------------------

/// Create `selector { }` at the end of the file when no top-level rule with
/// that selector exists.
pub fn ensure_rule(source: &str, selector: &str, options: ParseOptions) -> Result<Outcome> {
    ensure_rule_with(source, selector, "", options)
}

/// Same, but seeding the new rule with `declarations` when it has to be created.
pub fn ensure_rule_with(
    source: &str,
    selector: &str,
    declarations: &str,
    options: ParseOptions,
) -> Result<Outcome> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(CssError::InvalidInput("selector is empty".to_string()));
    }
    validate_snippet(selector, "selector")?;
    validate_snippet(declarations, "declarations")?;
    if selector.contains('{') || selector.contains('}') {
        return Err(CssError::InvalidInput(format!(
            "selector {selector:?} must not contain braces"
        )));
    }
    let body = split_declarations(declarations);

    run(source, options, |ctx| {
        match find_rule_by_selector(ctx, selector) {
            MatchResult::One(_) | MatchResult::Ambiguous(_) => Ok(vec![]),
            MatchResult::None => Ok(append_rule_edits(ctx, selector, &body)),
        }
    })
}

/// Remove a top-level rule and the comments it owns (rule D).
pub fn remove_rule(source: &str, selector: &str, options: ParseOptions) -> Result<Outcome> {
    let want = normalize_selector(selector);
    if want.is_empty() {
        return Err(CssError::InvalidInput("selector is empty".to_string()));
    }
    run(source, options, |ctx| {
        let comments = comment_ranges(ctx);
        let mut edits = Vec::new();
        // Removal is the one place ambiguity is harmless: "delete this rule"
        // means all of them, and deleting each one is itself unambiguous.
        for rule in find_top_level_rules(ctx) {
            if rule.selector_norm != want {
                continue;
            }
            let span = deletion_span(ctx, &comments, rule.start, rule.end);
            let span = absorb_surrounding_blank_line(ctx, span);
            edits.push(Edit::delete(span.start, span.end));
        }
        Ok(edits)
    })
}

/// Replace everything between a rule's braces with `declarations`.
pub fn replace_rule_body(
    source: &str,
    selector: &str,
    declarations: &str,
    options: ParseOptions,
) -> Result<Outcome> {
    validate_snippet(declarations, "declarations")?;
    let body = split_declarations(declarations);

    run(source, options, |ctx| {
        let Some(rule) = resolve_rule(ctx, selector)? else {
            return Err(CssError::NotFound(format!(
                "no top-level rule with selector {selector:?}"
            )));
        };
        let nl = ctx.nl();
        let rule_indent = ctx.indent_at(rule.start).to_string();
        let single_line = !ctx.source()[rule.start..rule.end].contains('\n');

        let replacement = if body.is_empty() {
            if single_line {
                String::new()
            } else {
                format!("{nl}{rule_indent}")
            }
        } else if single_line {
            format!(" {} ", body.join(" "))
        } else {
            let inner = body
                .iter()
                .map(|d| format!("{rule_indent}{}{d}", ctx.indent()))
                .collect::<Vec<_>>()
                .join(nl);
            format!("{nl}{inner}{nl}{rule_indent}")
        };
        Ok(vec![Edit::replace(
            rule.body_open,
            rule.body_close,
            replacement,
        )])
    })
}

/// Insert caller-provided raw text at the end of a rule body, re-indented to
/// match the surrounding code.
pub fn append_raw_to_rule(
    source: &str,
    selector: &str,
    raw: &str,
    options: ParseOptions,
) -> Result<Outcome> {
    validate_snippet(raw, "raw block")?;
    if raw.trim().is_empty() {
        return Err(CssError::InvalidInput("raw block is empty".to_string()));
    }

    run(source, options, |ctx| {
        let Some(rule) = resolve_rule(ctx, selector)? else {
            return Err(CssError::NotFound(format!(
                "no top-level rule with selector {selector:?}"
            )));
        };
        // Already present verbatim? Then this is a no-op (rule A).
        let body = &ctx.source()[rule.body_open..rule.body_close];
        let needle = raw.trim();
        if body.contains(needle) {
            return Ok(vec![]);
        }

        let rule_indent = ctx.indent_at(rule.start).to_string();
        let inner_indent = format!("{rule_indent}{}", ctx.indent());
        // `append_to_body` supplies the indentation of the first line itself, so
        // hand it a block whose first line is already flush.
        let text = reindent(needle, &inner_indent, ctx.nl())
            .trim_start()
            .to_string();
        Ok(append_to_body(ctx, &rule, &text))
    })
}

/// Read-only: does a top-level rule with this selector exist?
pub fn has_rule(source: &str, selector: &str, options: ParseOptions) -> Result<bool> {
    let want = normalize_selector(selector);
    crate::ops::query(source, options, |ctx| {
        Ok(find_top_level_rules(ctx)
            .iter()
            .any(|r| r.selector_norm == want))
    })
}

/// Read-only: every top-level selector, as written.
pub fn list_selectors(source: &str, options: ParseOptions) -> Result<Vec<String>> {
    crate::ops::query(source, options, |ctx| {
        Ok(find_top_level_rules(ctx)
            .into_iter()
            .map(|r| r.selector_raw)
            .collect())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> ParseOptions {
        ParseOptions::default()
    }

    // -- ensure_rule --------------------------------------------------------

    #[test]
    fn creates_a_missing_rule_at_the_end() {
        let o = ensure_rule(".a { color: red; }\n", ".b", opts()).unwrap();
        assert!(o.changed);
        assert_eq!(o.source, ".a { color: red; }\n\n.b {\n}\n");
    }

    #[test]
    fn creates_a_rule_in_an_empty_file() {
        let o = ensure_rule("", ".b", opts()).unwrap();
        assert_eq!(o.source, ".b {\n}\n");
    }

    #[test]
    fn does_not_recreate_an_existing_rule() {
        let src = ".b {\n  color: red;\n}\n";
        let o = ensure_rule(src, ".b", opts()).unwrap();
        assert!(!o.changed);
        assert_eq!(o.source, src);
    }

    #[test]
    fn matches_an_existing_rule_written_with_different_spacing() {
        let src = ".a   >   .b { color: red; }\n";
        let o = ensure_rule(src, ".a>.b", opts()).unwrap();
        assert!(!o.changed);
    }

    #[test]
    fn ensure_rule_is_idempotent() {
        let once = ensure_rule(".a {}\n", ".b", opts()).unwrap();
        let twice = ensure_rule(&once.source, ".b", opts()).unwrap();
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn seeds_a_new_rule_with_declarations() {
        let o = ensure_rule_with("", ".b", "color: red; margin: 0", opts()).unwrap();
        assert_eq!(o.source, ".b {\n  color: red;\n  margin: 0;\n}\n");
    }

    #[test]
    fn new_rules_follow_the_files_indent_and_newline_style() {
        let src = ".a {\r\n\tcolor: red;\r\n}\r\n";
        let o = ensure_rule_with(src, ".b", "margin: 0", opts()).unwrap();
        assert_eq!(
            o.source,
            ".a {\r\n\tcolor: red;\r\n}\r\n\r\n.b {\r\n\tmargin: 0;\r\n}\r\n"
        );
    }

    #[test]
    fn a_file_without_a_trailing_newline_keeps_not_having_one() {
        let o = ensure_rule(".a {}", ".b", opts()).unwrap();
        assert_eq!(o.source, ".a {}\n\n.b {\n}");
    }

    #[test]
    fn preserves_a_trailing_comment_when_appending() {
        let src = ".a {}\n\n/* the end */\n";
        let o = ensure_rule(src, ".b", opts()).unwrap();
        assert!(o.source.contains("/* the end */"));
        assert_eq!(o.source, ".a {}\n\n/* the end */\n\n.b {\n}\n");
    }

    #[test]
    fn rejects_a_selector_containing_braces() {
        assert!(ensure_rule("", ".a { }", opts()).is_err());
        assert!(ensure_rule("", "  ", opts()).is_err());
    }

    // -- remove_rule --------------------------------------------------------

    #[test]
    fn removes_a_rule_and_its_line() {
        let src = ".a {}\n.b {}\n.c {}\n";
        let o = remove_rule(src, ".b", opts()).unwrap();
        assert!(o.changed);
        assert_eq!(o.source, ".a {}\n.c {}\n");
    }

    #[test]
    fn removes_the_comment_directly_above() {
        let src = ".a {}\n\n/* about b */\n.b {}\n\n.c {}\n";
        let o = remove_rule(src, ".b", opts()).unwrap();
        assert_eq!(o.source, ".a {}\n\n.c {}\n");
    }

    #[test]
    fn keeps_a_section_header_above_a_removed_rule() {
        let src = "/* ===== Utilities ===== */\n.b {}\n.c {}\n";
        let o = remove_rule(src, ".b", opts()).unwrap();
        assert_eq!(o.source, "/* ===== Utilities ===== */\n.c {}\n");
    }

    #[test]
    fn removing_an_absent_rule_is_a_no_op() {
        let src = ".a {}\n";
        let o = remove_rule(src, ".zz", opts()).unwrap();
        assert!(!o.changed);
        assert_eq!(o.source, src);
    }

    #[test]
    fn removes_every_copy_of_a_duplicated_rule() {
        let src = ".a {}\n.b { color: red; }\n.b { color: blue; }\n";
        let o = remove_rule(src, ".b", opts()).unwrap();
        assert_eq!(o.source, ".a {}\n");
    }

    #[test]
    fn remove_rule_is_idempotent() {
        let src = ".a {}\n.b {}\n";
        let once = remove_rule(src, ".b", opts()).unwrap();
        let twice = remove_rule(&once.source, ".b", opts()).unwrap();
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn does_not_remove_a_nested_rule() {
        let src = "@media print {\n  .b { color: red; }\n}\n";
        let o = remove_rule(src, ".b", opts()).unwrap();
        assert!(!o.changed);
    }

    // -- replace_rule_body --------------------------------------------------

    #[test]
    fn replaces_a_multi_line_body() {
        let src = ".a {\n  color: red;\n  margin: 0;\n}\n";
        let o = replace_rule_body(src, ".a", "padding: 1px; color: blue", opts()).unwrap();
        assert_eq!(o.source, ".a {\n  padding: 1px;\n  color: blue;\n}\n");
    }

    #[test]
    fn replaces_a_single_line_body_in_place() {
        let src = ".a { color: red; }\n";
        let o = replace_rule_body(src, ".a", "color: blue", opts()).unwrap();
        assert_eq!(o.source, ".a { color: blue; }\n");
    }

    #[test]
    fn empties_a_body() {
        let src = ".a {\n  color: red;\n}\n";
        let o = replace_rule_body(src, ".a", "", opts()).unwrap();
        assert_eq!(o.source, ".a {\n}\n");
    }

    #[test]
    fn replacing_the_body_leaves_the_rest_of_the_file_alone() {
        let src = "/* head */\n.a {\n  color: red;\n}\n/* tail */\n.b {}\n";
        let o = replace_rule_body(src, ".a", "color: blue", opts()).unwrap();
        assert_eq!(
            o.source,
            "/* head */\n.a {\n  color: blue;\n}\n/* tail */\n.b {}\n"
        );
    }

    #[test]
    fn replace_body_errors_on_a_missing_rule() {
        let e = replace_rule_body(".a {}\n", ".zz", "color: red", opts()).unwrap_err();
        assert!(matches!(e, CssError::NotFound(_)));
    }

    #[test]
    fn replace_body_errors_on_an_ambiguous_selector() {
        let e = replace_rule_body(".a {}\n.a {}\n", ".a", "color: red", opts()).unwrap_err();
        assert!(matches!(e, CssError::AmbiguousSelector { count: 2, .. }));
    }

    #[test]
    fn replace_body_is_idempotent() {
        let src = ".a {\n  color: red;\n}\n";
        let once = replace_rule_body(src, ".a", "color: blue", opts()).unwrap();
        let twice = replace_rule_body(&once.source, ".a", "color: blue", opts()).unwrap();
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    // -- append_raw_to_rule -------------------------------------------------

    #[test]
    fn appends_raw_text_to_a_body() {
        let src = ".a {\n  color: red;\n}\n";
        let o = append_raw_to_rule(src, ".a", "margin: 0;", opts()).unwrap();
        assert_eq!(o.source, ".a {\n  color: red;\n  margin: 0;\n}\n");
    }

    #[test]
    fn appends_into_an_empty_body() {
        let src = ".a {\n}\n";
        let o = append_raw_to_rule(src, ".a", "margin: 0;", opts()).unwrap();
        assert_eq!(o.source, ".a {\n  margin: 0;\n}\n");
    }

    #[test]
    fn appends_into_an_inline_empty_body() {
        let src = ".a {}\n";
        let o = append_raw_to_rule(src, ".a", "margin: 0;", opts()).unwrap();
        assert_eq!(o.source, ".a { margin: 0; }\n");
    }

    #[test]
    fn reindents_a_multi_line_raw_block() {
        let src = ".a {\n  color: red;\n}\n";
        let o = append_raw_to_rule(src, ".a", "&:hover {\n  color: blue;\n}", opts()).unwrap();
        assert_eq!(
            o.source,
            ".a {\n  color: red;\n  &:hover {\n    color: blue;\n  }\n}\n"
        );
    }

    #[test]
    fn adds_a_missing_semicolon_to_the_previous_declaration() {
        let src = ".a {\n  color: red\n}\n";
        let o = append_raw_to_rule(src, ".a", "margin: 0;", opts()).unwrap();
        assert_eq!(o.source, ".a {\n  color: red;\n  margin: 0;\n}\n");
    }

    #[test]
    fn lands_after_a_trailing_comment_not_before_it() {
        let src = ".a {\n  color: red; /* note */\n}\n";
        let o = append_raw_to_rule(src, ".a", "margin: 0;", opts()).unwrap();
        assert_eq!(
            o.source,
            ".a {\n  color: red; /* note */\n  margin: 0;\n}\n"
        );
    }

    #[test]
    fn append_raw_is_idempotent() {
        let src = ".a {\n  color: red;\n}\n";
        let once = append_raw_to_rule(src, ".a", "margin: 0;", opts()).unwrap();
        let twice = append_raw_to_rule(&once.source, ".a", "margin: 0;", opts()).unwrap();
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn append_raw_rejects_unbalanced_text() {
        assert!(append_raw_to_rule(".a {}\n", ".a", "&:hover {", opts()).is_err());
        assert!(append_raw_to_rule(".a {}\n", ".a", "   ", opts()).is_err());
    }

    #[test]
    fn append_raw_uses_tabs_when_the_file_does() {
        let src = ".a {\n\tcolor: red;\n}\n";
        let o = append_raw_to_rule(src, ".a", "margin: 0;", opts()).unwrap();
        assert_eq!(o.source, ".a {\n\tcolor: red;\n\tmargin: 0;\n}\n");
    }

    // -- queries ------------------------------------------------------------

    #[test]
    fn has_rule_is_top_level_and_normalised() {
        assert!(has_rule(".a  >  .b {}\n", ".a>.b", opts()).unwrap());
        assert!(!has_rule("@media print { .b {} }\n", ".b", opts()).unwrap());
    }

    #[test]
    fn lists_selectors_as_written() {
        let src = ".a,\n.b { color: red; }\n#c {}\n";
        assert_eq!(
            list_selectors(src, opts()).unwrap(),
            vec![".a,\n.b".to_string(), "#c".to_string()]
        );
    }
}
