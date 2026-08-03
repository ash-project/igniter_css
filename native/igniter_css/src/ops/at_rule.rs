// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Top-level at-rule codemods: `@import`, `@plugin`, `@source`, `@layer`,
//! `@custom-variant` and friends.
//!
//! For these, the grammar barely matters: what we need is an *anchor offset*,
//! and even a bogus node provides one (R1, mitigation 3).

use crate::ctx::{ParseCtx, ParseOptions};
use crate::edit::Edit;
use crate::error::{CssError, Result};
use crate::locate::{
    find_at_rules_named, find_top_level_at_rules, top_level_nodes, top_of_file_anchor, AtRuleRef,
};
use crate::ops::{run, validate_snippet, Outcome};
use crate::trivia::{absorb_surrounding_blank_line, comment_ranges, deletion_span};
use biome_css_syntax::CssSyntaxKind;

/// At-rules that must appear before any style rule, in this order.
const PROLOGUE_FIRST: &[&str] = &["charset", "import", "use", "namespace"];

/// The parsed shape of a caller-supplied at-rule line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtRuleSpec {
    /// Lowercase name without `@`.
    pub name: String,
    /// Normalised prelude (whitespace collapsed).
    pub prelude: String,
    /// The quoted/`url()` target, unquoted, when the prelude has one.
    pub target: Option<String>,
    /// The exact text to insert, `;`-terminated when it has no block.
    pub text: String,
}

/// Collapse whitespace runs outside of strings.
fn collapse_ws(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut pending = false;
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' => {
                if pending && !out.is_empty() {
                    out.push(' ');
                }
                pending = false;
                out.push(c);
                let quote = c;
                let mut escaped = false;
                for q in chars.by_ref() {
                    out.push(q);
                    if escaped {
                        escaped = false;
                    } else if q == '\\' {
                        escaped = true;
                    } else if q == quote {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => pending = true,
            _ => {
                if pending && !out.is_empty() {
                    out.push(' ');
                }
                pending = false;
                out.push(c);
            }
        }
    }
    out.trim().to_string()
}

/// The first quoted string or `url(...)` in a prelude, unquoted.
pub fn at_rule_target(prelude: &str) -> Option<String> {
    let p = prelude.trim();
    let bytes = p.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                let quote = bytes[i];
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() {
                    if bytes[j] == b'\\' {
                        j += 2;
                        continue;
                    }
                    if bytes[j] == quote {
                        return p.get(start..j).map(|s| s.to_string());
                    }
                    j += 1;
                }
                return None;
            }
            _ => {
                if p[i..].starts_with("url(") {
                    let start = i + 4;
                    let rest = &p[start..];
                    let end = rest.find(')')?;
                    let inner = rest[..end].trim();
                    let inner = inner
                        .trim_start_matches(['"', '\''])
                        .trim_end_matches(['"', '\'']);
                    return Some(inner.to_string());
                }
                i += 1;
            }
        }
    }
    None
}

/// Parse a caller-supplied at-rule line such as `@plugin "daisyui";`.
pub fn parse_at_rule_spec(line: &str) -> Result<AtRuleSpec> {
    let trimmed = line.trim();
    if !trimmed.starts_with('@') {
        return Err(CssError::InvalidInput(format!(
            "at-rule line must start with `@`, got {trimmed:?}"
        )));
    }
    validate_snippet(trimmed, "at-rule line")?;

    let has_block = trimmed.contains('{');
    let text = if has_block || trimmed.ends_with(';') {
        trimmed.to_string()
    } else {
        format!("{trimmed};")
    };

    let ctx = ParseCtx::new(&text, ParseOptions::default());
    if !ctx.round_trips() {
        return Err(CssError::InvalidInput(format!(
            "cannot understand at-rule line {trimmed:?}"
        )));
    }
    let rules = find_top_level_at_rules(&ctx);
    let Some(rule) = rules.first() else {
        return Err(CssError::InvalidInput(format!(
            "{trimmed:?} is not an at-rule"
        )));
    };
    if rules.len() > 1 {
        return Err(CssError::InvalidInput(
            "expected exactly one at-rule".to_string(),
        ));
    }

    let prelude = collapse_ws(&rule.prelude);
    Ok(AtRuleSpec {
        name: rule.name.clone(),
        target: at_rule_target(&prelude),
        prelude,
        text,
    })
}

/// Is `existing` the same at-rule as `spec`, for idempotency purposes?
///
/// Two at-rules of the same name that name the same target are the same rule --
/// `@import "tailwindcss";` and `@import "tailwindcss" source(none);` are not
/// two imports of two different things, they are one import written twice.
fn is_equivalent(spec: &AtRuleSpec, existing: &AtRuleRef) -> bool {
    if existing.name != spec.name {
        return false;
    }
    let existing_prelude = collapse_ws(&existing.prelude);
    match (&spec.target, at_rule_target(&existing_prelude)) {
        (Some(a), Some(b)) => a == &b,
        _ => existing_prelude == spec.prelude,
    }
}

/// Where a new at-rule of this family should be inserted.
fn insertion_offset(ctx: &ParseCtx, spec: &AtRuleSpec) -> usize {
    let comments = comment_ranges(ctx);

    // 1. After the last at-rule with the same name.
    if let Some(last) = find_at_rules_named(ctx, &spec.name).last() {
        return crate::ops::past_trailing_comment(ctx, &comments, last.end);
    }

    // 2. `@charset`/`@import`/`@namespace` must precede style rules, so they go
    //    after the last at-rule that also belongs at the top, and never after a
    //    style rule.
    let is_prologue_first = PROLOGUE_FIRST.contains(&spec.name.as_str());

    let mut anchor: Option<usize> = None;
    for node in top_level_nodes(ctx) {
        match node.kind() {
            CssSyntaxKind::CSS_AT_RULE => {
                let Some(at) = find_top_level_at_rules(ctx)
                    .into_iter()
                    .find(|r| r.node == node)
                else {
                    continue;
                };
                if is_prologue_first && !PROLOGUE_FIRST.contains(&at.name.as_str()) {
                    break;
                }
                anchor = Some(at.end);
            }
            // A style rule ends the prologue.
            CssSyntaxKind::CSS_QUALIFIED_RULE => break,
            _ => {}
        }
    }

    match anchor {
        Some(end) => crate::ops::past_trailing_comment(ctx, &comments, end),
        None => top_of_file_anchor(ctx),
    }
}

/// Insert `line` at the top level unless an equivalent at-rule is already
/// present (ROADMAP §8, `ensure_at_rule_line`).
pub fn ensure_at_rule_line(source: &str, line: &str, options: ParseOptions) -> Result<Outcome> {
    let spec = parse_at_rule_spec(line)?;
    run(source, options, |ctx| {
        if find_top_level_at_rules(ctx)
            .iter()
            .any(|r| is_equivalent(&spec, r))
        {
            return Ok(vec![]);
        }

        let at = insertion_offset(ctx, &spec);
        let nl = ctx.nl();
        let indent = ctx.indent_at(at);

        // Insert on its own line, keeping the surrounding line structure.
        let text = if at == 0 {
            format!("{}{nl}", spec.text)
        } else if ctx.source()[..at].ends_with('\n') {
            format!("{indent}{}{nl}", spec.text)
        } else {
            format!("{nl}{indent}{}", spec.text)
        };
        Ok(vec![Edit::insert(at, text)])
    })
}

/// Remove every top-level at-rule of `name` whose target (or, failing that,
/// whose whole prelude) matches `matching`. `matching` of `None` removes all of
/// them.
pub fn remove_at_rule(
    source: &str,
    name: &str,
    matching: Option<&str>,
    options: ParseOptions,
) -> Result<Outcome> {
    let want_name = name.trim_start_matches('@').to_lowercase();
    let want = matching.map(collapse_ws);

    run(source, options, |ctx| {
        let comments = comment_ranges(ctx);
        let mut edits = Vec::new();
        for at in find_top_level_at_rules(ctx) {
            if at.name != want_name {
                continue;
            }
            if let Some(w) = &want {
                let prelude = collapse_ws(&at.prelude);
                let hit = match (at_rule_target(&prelude), at_rule_target(w)) {
                    (Some(a), Some(b)) => a == b,
                    (Some(a), None) => a == *w,
                    _ => prelude == *w,
                };
                if !hit {
                    continue;
                }
            }
            let span = deletion_span(ctx, &comments, at.start, at.end);
            let span = absorb_surrounding_blank_line(ctx, span);
            edits.push(Edit::delete(span.start, span.end));
        }
        Ok(edits)
    })
}

/// `@import` convenience wrapper: builds the line and delegates.
pub fn add_import(
    source: &str,
    url: &str,
    media: Option<&str>,
    options: ParseOptions,
) -> Result<Outcome> {
    let url = url.trim();
    if url.is_empty() {
        return Err(CssError::InvalidInput("import url is empty".to_string()));
    }
    if url.contains('"') || url.contains('\n') {
        return Err(CssError::InvalidInput(format!(
            "import url {url:?} contains characters that cannot be quoted safely"
        )));
    }
    // Absolute URLs read better as `url(...)`; relative paths as a plain string.
    let target =
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with('/') {
            format!("url(\"{url}\")")
        } else {
            format!("\"{url}\"")
        };
    let line = match media.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => format!("@import {target} {m};"),
        None => format!("@import {target};"),
    };
    ensure_at_rule_line(source, &line, options)
}

pub fn remove_import(source: &str, url: &str, options: ParseOptions) -> Result<Outcome> {
    remove_at_rule(source, "import", Some(url), options)
}

/// Read-only: is an equivalent at-rule already present?
pub fn has_at_rule(source: &str, line: &str, options: ParseOptions) -> Result<bool> {
    let spec = parse_at_rule_spec(line)?;
    crate::ops::query(source, options, |ctx| {
        Ok(find_top_level_at_rules(ctx)
            .iter()
            .any(|r| is_equivalent(&spec, r)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure(src: &str, line: &str) -> Outcome {
        ensure_at_rule_line(src, line, ParseOptions::default()).unwrap()
    }

    fn remove(src: &str, name: &str, matching: Option<&str>) -> Outcome {
        remove_at_rule(src, name, matching, ParseOptions::default()).unwrap()
    }

    // -- spec parsing -------------------------------------------------------

    #[test]
    fn parses_a_simple_at_rule_line() {
        let s = parse_at_rule_spec("@plugin \"daisyui\";").unwrap();
        assert_eq!(s.name, "plugin");
        assert_eq!(s.target.as_deref(), Some("daisyui"));
        assert_eq!(s.text, "@plugin \"daisyui\";");
    }

    #[test]
    fn adds_a_missing_semicolon() {
        let s = parse_at_rule_spec("@source \"../js\"").unwrap();
        assert_eq!(s.text, "@source \"../js\";");
    }

    #[test]
    fn parses_a_url_target() {
        let s = parse_at_rule_spec("@import url(\"/a/b.css\");").unwrap();
        assert_eq!(s.target.as_deref(), Some("/a/b.css"));
    }

    #[test]
    fn parses_a_block_at_rule() {
        let s = parse_at_rule_spec("@plugin \"x\" { themes: false; }").unwrap();
        assert_eq!(s.name, "plugin");
        assert!(s.text.ends_with('}'));
    }

    #[test]
    fn rejects_text_that_is_not_an_at_rule() {
        assert!(parse_at_rule_spec(".a { color: red; }").is_err());
        assert!(parse_at_rule_spec("").is_err());
    }

    #[test]
    fn rejects_an_unbalanced_at_rule_line() {
        assert!(parse_at_rule_spec("@plugin \"x\" {").is_err());
    }

    // -- insertion ----------------------------------------------------------

    #[test]
    fn inserts_into_an_empty_file() {
        let o = ensure("", "@import \"tailwindcss\";");
        assert!(o.changed);
        assert_eq!(o.source, "@import \"tailwindcss\";\n");
    }

    #[test]
    fn inserts_after_the_last_at_rule_of_the_same_name() {
        let src = "@import \"a\";\n@import \"b\";\n\n.x { color: red; }\n";
        let o = ensure(src, "@import \"c\";");
        assert_eq!(
            o.source,
            "@import \"a\";\n@import \"b\";\n@import \"c\";\n\n.x { color: red; }\n"
        );
    }

    #[test]
    fn inserts_at_the_end_of_the_prologue_when_the_family_is_new() {
        let src = "@import \"tailwindcss\";\n@source \"../js\";\n\n.x { color: red; }\n";
        let o = ensure(src, "@plugin \"daisyui\";");
        assert_eq!(
            o.source,
            "@import \"tailwindcss\";\n@source \"../js\";\n@plugin \"daisyui\";\n\n.x { color: red; }\n"
        );
    }

    #[test]
    fn an_import_never_lands_after_a_style_rule() {
        let src = ".x { color: red; }\n@plugin \"a\";\n";
        let o = ensure(src, "@import \"b\";");
        assert!(o.source.starts_with("@import \"b\";\n.x"));
    }

    #[test]
    fn a_plugin_lands_after_the_prologue_not_before_it() {
        let src = "@charset \"utf-8\";\n@import \"a\";\n.x { color: red; }\n";
        let o = ensure(src, "@plugin \"p\";");
        assert_eq!(
            o.source,
            "@charset \"utf-8\";\n@import \"a\";\n@plugin \"p\";\n.x { color: red; }\n"
        );
    }

    #[test]
    fn inserts_below_a_file_header_comment() {
        let src = "/* App styles.\n   Two lines. */\n\n.x { color: red; }\n";
        let o = ensure(src, "@import \"a\";");
        assert_eq!(
            o.source,
            "/* App styles.\n   Two lines. */\n@import \"a\";\n\n.x { color: red; }\n"
        );
    }

    #[test]
    fn inserts_above_a_comment_that_documents_the_first_rule() {
        let src = "/* about .x */\n.x { color: red; }\n";
        let o = ensure(src, "@import \"a\";");
        assert_eq!(
            o.source,
            "/* about .x */\n@import \"a\";\n.x { color: red; }\n"
        );
    }

    #[test]
    fn uses_the_files_newline_style() {
        let src = "@import \"a\";\r\n.x { color: red; }\r\n";
        let o = ensure(src, "@import \"b\";");
        assert_eq!(
            o.source,
            "@import \"a\";\r\n@import \"b\";\r\n.x { color: red; }\r\n"
        );
    }

    #[test]
    fn handles_a_file_with_no_trailing_newline() {
        let o = ensure(".x { color: red; }", "@import \"a\";");
        assert_eq!(o.source, "@import \"a\";\n.x { color: red; }");
    }

    #[test]
    fn preserves_a_bom() {
        let o = ensure("\u{feff}.x { color: red; }\n", "@import \"a\";");
        assert_eq!(o.source, "\u{feff}@import \"a\";\n.x { color: red; }\n");
    }

    // -- idempotency --------------------------------------------------------

    #[test]
    fn is_idempotent() {
        let src = "@import \"a\";\n.x { color: red; }\n";
        let once = ensure(src, "@plugin \"p\";");
        let twice = ensure(&once.source, "@plugin \"p\";");
        assert!(once.changed);
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn an_existing_rule_with_the_same_target_counts_as_present() {
        let src = "@import \"tailwindcss\" source(none);\n";
        let o = ensure(src, "@import \"tailwindcss\";");
        assert!(!o.changed);
        assert_eq!(o.source, src);
    }

    #[test]
    fn quoting_style_does_not_create_a_duplicate() {
        let src = "@plugin '../vendor/daisyui';\n";
        let o = ensure(src, "@plugin \"../vendor/daisyui\";");
        assert!(!o.changed);
    }

    #[test]
    fn a_different_target_is_added() {
        let src = "@plugin \"a\";\n";
        let o = ensure(src, "@plugin \"b\";");
        assert!(o.changed);
        assert_eq!(o.source, "@plugin \"a\";\n@plugin \"b\";\n");
    }

    #[test]
    fn has_at_rule_agrees_with_ensure() {
        let src = "@plugin \"a\";\n";
        assert!(has_at_rule(src, "@plugin \"a\";", ParseOptions::default()).unwrap());
        assert!(!has_at_rule(src, "@plugin \"b\";", ParseOptions::default()).unwrap());
    }

    // -- comment preservation ----------------------------------------------

    #[test]
    fn insertion_keeps_every_comment() {
        let src = "/* one */\n@import \"a\"; /* two */\n/* three */\n.x { color: red; }\n";
        let o = ensure(src, "@import \"b\";");
        for c in ["/* one */", "/* two */", "/* three */"] {
            assert!(o.source.contains(c), "lost {c}");
        }
        assert_eq!(
            o.source,
            "/* one */\n@import \"a\"; /* two */\n@import \"b\";\n/* three */\n.x { color: red; }\n"
        );
    }

    // -- removal ------------------------------------------------------------

    #[test]
    fn removes_a_matching_at_rule() {
        let src = "@import \"a\";\n@import \"b\";\n.x { color: red; }\n";
        let o = remove(src, "import", Some("a"));
        assert!(o.changed);
        assert_eq!(o.source, "@import \"b\";\n.x { color: red; }\n");
    }

    #[test]
    fn removes_every_at_rule_of_a_name_when_unfiltered() {
        let src = "@import \"a\";\n@import \"b\";\n.x { color: red; }\n";
        let o = remove(src, "import", None);
        assert_eq!(o.source, ".x { color: red; }\n");
    }

    #[test]
    fn removing_something_absent_is_a_no_op() {
        let src = ".x { color: red; }\n";
        let o = remove(src, "import", Some("a"));
        assert!(!o.changed);
        assert_eq!(o.source, src);
    }

    #[test]
    fn removal_takes_the_adjacent_comment_but_not_the_header() {
        let src = "/* ===== Imports ===== */\n/* the app css */\n@import \"a\";\n@import \"b\";\n";
        let o = remove(src, "import", Some("a"));
        assert_eq!(o.source, "/* ===== Imports ===== */\n@import \"b\";\n");
    }

    #[test]
    fn removal_is_idempotent() {
        let src = "@import \"a\";\n@import \"b\";\n";
        let once = remove(src, "import", Some("a"));
        let twice = remove(&once.source, "import", Some("a"));
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn remove_import_matches_a_url_written_either_way() {
        let src = "@import url(\"/a.css\");\n@import \"b\";\n";
        let o = remove_import(src, "/a.css", ParseOptions::default()).unwrap();
        assert_eq!(o.source, "@import \"b\";\n");
    }

    // -- add_import ---------------------------------------------------------

    #[test]
    fn add_import_quotes_a_relative_path() {
        let o = add_import("", "styles.css", None, ParseOptions::default()).unwrap();
        assert_eq!(o.source, "@import \"styles.css\";\n");
    }

    #[test]
    fn add_import_wraps_an_absolute_url() {
        let o = add_import("", "https://x/y.css", None, ParseOptions::default()).unwrap();
        assert_eq!(o.source, "@import url(\"https://x/y.css\");\n");
    }

    #[test]
    fn add_import_carries_a_media_query() {
        let o = add_import(
            "",
            "m.css",
            Some("screen and (max-width: 768px)"),
            ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(
            o.source,
            "@import \"m.css\" screen and (max-width: 768px);\n"
        );
    }

    #[test]
    fn add_import_is_idempotent_across_media_queries() {
        let src = "@import \"m.css\" screen;\n";
        let o = add_import(src, "m.css", None, ParseOptions::default()).unwrap();
        assert!(!o.changed);
    }

    #[test]
    fn add_import_rejects_an_unquotable_url() {
        assert!(add_import("", "a\"b", None, ParseOptions::default()).is_err());
        assert!(add_import("", "  ", None, ParseOptions::default()).is_err());
    }

    // -- targets ------------------------------------------------------------

    #[test]
    fn extracts_targets_from_preludes() {
        assert_eq!(at_rule_target("\"a/b.css\""), Some("a/b.css".into()));
        assert_eq!(at_rule_target("'a'"), Some("a".into()));
        assert_eq!(at_rule_target("url(\"x\")"), Some("x".into()));
        assert_eq!(at_rule_target("url(x)"), Some("x".into()));
        assert_eq!(at_rule_target("base, components"), None);
    }
}
