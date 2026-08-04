// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! **Whole-file transforms. These are not codemods.**
//!
//! Everything in [`crate::ops`] is diff-minimal by construction. The functions
//! here deliberately are not: minifying or beautifying rewrites every byte, and
//! minifying discards comments because that is what minifying *is*.
//!
//! Keep them out of Igniter installers. They exist for build-time and reporting
//! use, they are never used to patch a user's file in place, and the codemods
//! never route their output through here.

use crate::ctx::{ParseCtx, ParseOptions};
use crate::error::Result;
use crate::locate::all_comments;
use biome_css_syntax::CssSyntaxKind;
use biome_rowan::Direction;

/// Did the author leave a blank line in this run of whitespace?
fn blank_line_in(gap: &str) -> bool {
    gap.matches('\n').count() >= 2
}

/// Characters that would merge with an adjacent one if the space between them
/// were dropped.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '%' | '.' | '#' | '\\') || (c as u32) >= 128
}

/// Strip comments and collapse whitespace.
///
/// Driven by the token stream rather than by regex over the text, so a `;`
/// inside `url(...)` or a `/*` inside a string is never mistaken for syntax. A
/// space is preserved only where the source had whitespace **and** removing it
/// would change how the result tokenises -- so `and (min-width: 1px)` keeps its
/// space while `url(x)` never gains one.
pub fn minify(source: &str, options: ParseOptions) -> Result<String> {
    crate::ctx::check_nesting(source)?;
    let ctx = ParseCtx::try_new(source, options)?;
    let mut out = String::with_capacity(source.len());
    let mut prev_end: Option<usize> = None;

    let tokens: Vec<_> = ctx
        .syntax()
        .descendants_tokens(Direction::Next)
        .filter(|t| t.kind() != CssSyntaxKind::EOF)
        .collect();

    for (i, token) in tokens.iter().enumerate() {
        let text = token.text_trimmed();
        if text.is_empty() {
            continue;
        }
        let start = usize::from(token.text_trimmed_range().start());
        let end = usize::from(token.text_trimmed_range().end());

        // Drop the semicolon that terminates the last declaration in a block.
        if token.kind() == CssSyntaxKind::SEMICOLON {
            let next_is_close = tokens
                .get(i + 1)
                .is_some_and(|t| t.kind() == CssSyntaxKind::R_CURLY);
            if next_is_close {
                prev_end = Some(end);
                continue;
            }
        }

        if let (Some(pe), Some(last)) = (prev_end, out.chars().last()) {
            let gap = ctx.source().get(pe..start).unwrap_or("");
            let first = text.chars().next().unwrap_or(' ');
            let had_space = !gap.is_empty();
            // `(` matters: `and (` must not become `and(`, which would read as
            // a function call, but `url(` must never gain a space.
            let would_merge = is_word_char(last) && (is_word_char(first) || first == '(');
            if had_space && would_merge {
                out.push(' ');
            }
        }

        out.push_str(text);
        prev_end = Some(end);
    }

    Ok(ctx.restore_bom(out))
}

/// Re-print the stylesheet with one declaration per line and consistent
/// indentation, keeping every comment.
///
/// A conventional pretty-printer: whole-file output, so never use it to patch.
pub fn beautify(source: &str, options: ParseOptions) -> Result<String> {
    crate::ctx::check_nesting(source)?;
    let ctx = ParseCtx::try_new(source, options)?;
    let nl = ctx.nl();
    let unit = ctx.indent();
    let comments = all_comments(&ctx);

    let mut out = String::with_capacity(source.len());
    let mut depth = 0usize;
    let mut prev_end: Option<usize> = None;
    let mut at_line_start = true;

    let tokens: Vec<_> = ctx
        .syntax()
        .descendants_tokens(Direction::Next)
        .filter(|t| t.kind() != CssSyntaxKind::EOF)
        .collect();

    // Emit comments in stream order alongside the tokens they precede.
    let mut comment_idx = 0usize;
    // Whether the last thing written was a comment, which decides whether a
    // repaired `;` would land inside it.
    let mut last_was_comment = false;

    let push = |out: &mut String, at_line_start: &mut bool, depth: usize, text: &str| {
        if *at_line_start {
            for _ in 0..depth {
                out.push_str(unit);
            }
            *at_line_start = false;
        }
        out.push_str(text);
    };

    for token in &tokens {
        let start = usize::from(token.text_trimmed_range().start());
        let text = token.text_trimmed();
        if text.is_empty() {
            continue;
        }

        // Any comment that sits before this token goes out first.
        while comment_idx < comments.len() && comments[comment_idx].0 < start {
            let (c_start, c_end, c_text) = &comments[comment_idx];
            comment_idx += 1;
            let gap = prev_end
                .and_then(|pe| ctx.source().get(pe..*c_start))
                .unwrap_or("");
            let same_line = prev_end.is_some() && !gap.contains('\n');

            if same_line && !at_line_start {
                out.push(' ');
                out.push_str(c_text);
                // A `//` comment runs to end of line: anything we emit after it
                // on the same line would be silently commented out.
                if c_text.starts_with("//") {
                    out.push_str(nl);
                    at_line_start = true;
                }
            } else {
                if !at_line_start {
                    out.push_str(nl);
                    at_line_start = true;
                }
                if blank_line_in(gap) && !out.is_empty() {
                    out.push_str(nl);
                }
                // A comment sitting just before `}` still belongs to the block
                // body, so it keeps body indentation rather than dedenting.
                push(&mut out, &mut at_line_start, depth, c_text);
                out.push_str(nl);
                at_line_start = true;
            }
            last_was_comment = true;
            // Anchor the next gap at the comment, not at the token before it,
            // or the newline the comment already consumed reads as a blank line.
            prev_end = Some(*c_end);
        }

        // Preserve a single blank line the author put between constructs.
        if at_line_start && !out.is_empty() && token.kind() != CssSyntaxKind::R_CURLY {
            let gap = prev_end
                .and_then(|pe| ctx.source().get(pe..start))
                .unwrap_or("");
            if blank_line_in(gap) && !out.ends_with(&format!("{nl}{nl}")) {
                out.push_str(nl);
            }
        }

        match token.kind() {
            CssSyntaxKind::L_CURLY => {
                if !at_line_start && !out.ends_with(' ') {
                    out.push(' ');
                }
                push(&mut out, &mut at_line_start, depth, "{");
                depth += 1;
                out.push_str(nl);
                at_line_start = true;
            }
            CssSyntaxKind::R_CURLY => {
                // Minified input has no `;` on the last declaration of a block;
                // a pretty-printer should put one back.
                let content = out.trim_end();
                if !(last_was_comment
                    || content.is_empty()
                    || content.ends_with('{')
                    || content.ends_with('}')
                    || content.ends_with(';'))
                {
                    let at = content.len();
                    out.insert(at, ';');
                }
                if !at_line_start {
                    out.push_str(nl);
                    at_line_start = true;
                }
                depth = depth.saturating_sub(1);
                push(&mut out, &mut at_line_start, depth, "}");
                out.push_str(nl);
                at_line_start = true;
            }
            CssSyntaxKind::SEMICOLON => {
                push(&mut out, &mut at_line_start, depth, ";");
                out.push_str(nl);
                at_line_start = true;
            }
            CssSyntaxKind::COMMA => {
                push(&mut out, &mut at_line_start, depth, ",");
                out.push(' ');
            }
            CssSyntaxKind::COLON => {
                push(&mut out, &mut at_line_start, depth, ":");
                out.push(' ');
            }
            _ => {
                if !at_line_start {
                    let last = out.chars().last().unwrap_or(' ');
                    let first = text.chars().next().unwrap_or(' ');
                    let gap = prev_end
                        .and_then(|pe| ctx.source().get(pe..start))
                        .unwrap_or("");
                    let needs_space = !last.is_whitespace()
                        && ((!gap.is_empty()
                            && is_word_char(last)
                            && (is_word_char(first) || first == '('))
                            || matches!(first, '{'));
                    if needs_space {
                        out.push(' ');
                    }
                }
                push(&mut out, &mut at_line_start, depth, text);
            }
        }
        last_was_comment = false;
        prev_end = Some(usize::from(token.text_trimmed_range().end()));
    }

    // Trailing comments after the last token.
    while comment_idx < comments.len() {
        let (_, _, c_text) = &comments[comment_idx];
        comment_idx += 1;
        if !at_line_start {
            out.push_str(nl);
            at_line_start = true;
        }
        push(&mut out, &mut at_line_start, 0, c_text);
        out.push_str(nl);
        at_line_start = true;
    }

    let out = out.trim_end_matches(['\n', '\r', ' ', '\t']).to_string();
    let out = if out.is_empty() {
        out
    } else {
        format!("{out}{nl}")
    };
    Ok(ctx.restore_bom(out))
}

/// Concatenate stylesheets, then drop rules made redundant by a later copy.
pub fn merge_stylesheets(sheets: &[String], options: ParseOptions) -> Result<String> {
    let nl = "\n";
    let joined = sheets
        .iter()
        .map(|s| s.trim_matches(['\n', '\r']).to_string())
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join(&format!("{nl}{nl}"));

    if joined.trim().is_empty() {
        return Ok(String::new());
    }
    let joined = format!("{joined}{nl}");

    let deduped = crate::ops::tidy::remove_duplicates(
        &joined,
        crate::ops::tidy::DedupeOptions {
            declarations: false,
            rules: true,
        },
        options,
    )?;
    Ok(deduped.source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> ParseOptions {
        ParseOptions::default()
    }

    fn min(src: &str) -> String {
        minify(src, opts()).unwrap()
    }

    fn pretty(src: &str) -> String {
        beautify(src, opts()).unwrap()
    }

    // -- minify -------------------------------------------------------------

    #[test]
    fn minifies_a_simple_sheet() {
        let src =
            ".header {\n  color: #333;\n  background: #fff;\n}\n\n.footer {\n  color: #000;\n}\n";
        assert_eq!(
            min(src),
            ".header{color:#333;background:#fff}.footer{color:#000}"
        );
    }

    #[test]
    fn minifying_drops_comments() {
        let src = "/* a */\n.x { /* b */ color: red; /* c */ }\n";
        assert_eq!(min(src), ".x{color:red}");
    }

    #[test]
    fn minifying_keeps_a_space_the_grammar_needs() {
        let src = "@media screen and (min-width: 40em) {\n  .a { margin: 1px -2px; }\n}\n";
        assert_eq!(
            min(src),
            "@media screen and (min-width:40em){.a{margin:1px -2px}}"
        );
    }

    #[test]
    fn minifying_never_adds_a_space_before_a_function_paren() {
        let src = ".a {\n  background: url(a.png);\n  transform: translate(1px) rotate(2deg);\n}\n";
        assert_eq!(
            min(src),
            ".a{background:url(a.png);transform:translate(1px)rotate(2deg)}"
        );
    }

    #[test]
    fn minifying_preserves_string_contents() {
        let src = ".a::after {\n  content: \"a  b /* not a comment */ ;\";\n}\n";
        assert_eq!(
            min(src),
            ".a::after{content:\"a  b /* not a comment */ ;\"}"
        );
    }

    #[test]
    fn minifying_preserves_non_ascii() {
        let src = ".a::after {\n  content: \"日本語 ✓\";\n}\n";
        assert_eq!(min(src), ".a::after{content:\"日本語 ✓\"}");
    }

    #[test]
    fn minifying_collapses_selector_combinators() {
        let src = ".a  >  .b,\n.c {\n  color: red;\n}\n";
        assert_eq!(min(src), ".a>.b,.c{color:red}");
    }

    #[test]
    fn minifying_is_idempotent() {
        let src = ".a {\n  color: red;\n}\n";
        let once = min(src);
        assert_eq!(min(&once), once);
    }

    #[test]
    fn minifying_an_empty_sheet_yields_an_empty_string() {
        assert_eq!(min(""), "");
        assert_eq!(min("/* only a comment */\n"), "");
    }

    #[test]
    fn minified_output_still_parses() {
        let src = "@media print {\n  .a, .b > .c {\n    margin: 0 auto !important;\n  }\n}\n";
        let out = min(src);
        let ctx = ParseCtx::parse_default(&out);
        assert!(ctx.round_trips());
        assert!(!ctx.has_errors(), "minified output must still parse: {out}");
    }

    #[test]
    fn minifying_preserves_a_bom() {
        assert_eq!(min("\u{feff}.a { color: red; }\n"), "\u{feff}.a{color:red}");
    }

    // -- beautify -----------------------------------------------------------

    #[test]
    fn beautifies_a_minified_sheet() {
        let src = ".a{color:red;background:#fff}.b{color:#000}";
        assert_eq!(
            pretty(src),
            ".a {\n  color: red;\n  background: #fff;\n}\n.b {\n  color: #000;\n}\n"
        );
    }

    #[test]
    fn beautifying_keeps_comments() {
        let src = "/* head */.a{color:red}";
        assert_eq!(pretty(src), "/* head */\n.a {\n  color: red;\n}\n");
    }

    #[test]
    fn beautifying_indents_nested_blocks() {
        let src = "@media print{.a{color:red}}";
        assert_eq!(
            pretty(src),
            "@media print {\n  .a {\n    color: red;\n  }\n}\n"
        );
    }

    #[test]
    fn beautified_output_still_parses() {
        let src = ".a{color:red}@media print{.b{margin:0 auto!important}}";
        let out = pretty(src);
        let ctx = ParseCtx::parse_default(&out);
        assert!(ctx.round_trips());
        assert!(!ctx.has_errors(), "beautified output must parse: {out}");
    }

    #[test]
    fn beautifying_is_idempotent() {
        let src = ".a{color:red;margin:0}@media print{.b{color:blue}}";
        let once = pretty(src);
        assert_eq!(pretty(&once), once);
    }

    #[test]
    fn beautifying_an_empty_sheet_yields_an_empty_string() {
        assert_eq!(pretty(""), "");
    }

    #[test]
    fn beautify_then_minify_round_trips_to_the_same_minified_text() {
        let src = ".a{color:red;margin:0 auto}.b,.c>.d{padding:0}";
        assert_eq!(min(&pretty(src)), src);
    }

    // -- merge --------------------------------------------------------------

    #[test]
    fn merges_two_sheets() {
        let out = merge_stylesheets(
            &[".a { color: red; }".into(), ".b { color: blue; }".into()],
            opts(),
        )
        .unwrap();
        assert_eq!(out, ".a { color: red; }\n\n.b { color: blue; }\n");
    }

    #[test]
    fn merging_drops_an_identical_repeated_rule() {
        let out = merge_stylesheets(
            &[".a { color: red; }".into(), ".a { color: red; }".into()],
            opts(),
        )
        .unwrap();
        assert_eq!(out, ".a { color: red; }\n");
    }

    #[test]
    fn merging_keeps_a_later_rule_that_overrides() {
        let out = merge_stylesheets(
            &[".a { color: red; }".into(), ".a { color: blue; }".into()],
            opts(),
        )
        .unwrap();
        assert!(out.contains("color: red"));
        assert!(out.contains("color: blue"));
    }

    #[test]
    fn merging_preserves_comments() {
        let out = merge_stylesheets(
            &["/* one */\n.a {}".into(), "/* two */\n.b {}".into()],
            opts(),
        )
        .unwrap();
        assert!(out.contains("/* one */"));
        assert!(out.contains("/* two */"));
    }

    #[test]
    fn merging_skips_empty_sheets() {
        let out = merge_stylesheets(&["".into(), ".a {}".into(), "   ".into()], opts()).unwrap();
        assert_eq!(out, ".a {}\n");
    }

    #[test]
    fn merging_nothing_yields_an_empty_string() {
        assert_eq!(merge_stylesheets(&[], opts()).unwrap(), "");
    }
}
