// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Top-level at-rule codemods: `@import`, `@plugin`, `@source`, `@layer`,
//! `@custom-variant` and friends.
//!
//! For these, the grammar barely matters: what we need is an *anchor offset*,
//! and even a node Biome could not parse still provides one.

use crate::ctx::{ParseCtx, ParseOptions};
use crate::edit::Edit;
use crate::error::{CssError, Result};
use crate::locate::{
    at_rule_block_rule, declarations_in, find_at_rules_named, find_top_level_at_rules,
    find_top_level_rules, non_declaration_items, normalize_property, top_level_nodes,
    top_of_file_anchor, value_norm, AtRuleRef, DeclRef,
};
use crate::ops::rule::append_all_to_body;
use crate::ops::{reindent, run, validate_snippet, Outcome};
use crate::trivia::{absorb_surrounding_blank_line, comment_ranges, deletion_span};
use biome_css_syntax::CssSyntaxKind;
use std::collections::HashMap;

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

/// Normalise a caller-supplied needle (e.g. the `matching` argument of
/// [`remove_at_rule`]) into the same shape as an AST-derived target.
///
/// This is the one place we touch text rather than tokens, and deliberately so:
/// the argument is a bare path from Elixir, not CSS. Targets read *out of a
/// stylesheet* always come from `AtRuleRef::target`, which is token-derived.
pub fn normalize_target_needle(needle: &str) -> String {
    let n = needle.trim();
    // Tolerate a caller writing `url("x")` or `"x"` instead of just `x`.
    if let Some(rest) = n.strip_prefix("url(") {
        if let Some(inner) = rest.strip_suffix(')') {
            return crate::locate::unquote(inner);
        }
    }
    crate::locate::unquote(n)
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
    crate::ctx::check_nesting(trimmed)?;

    let has_block = trimmed.contains('{');
    let text = if has_block || trimmed.ends_with(';') {
        trimmed.to_string()
    } else {
        format!("{trimmed};")
    };

    let ctx = ParseCtx::try_new(&text, ParseOptions::default())?;
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

    let prelude = rule.prelude_norm.clone();
    Ok(AtRuleSpec {
        name: rule.name.clone(),
        // Read from the parsed line's own CST, not scanned out of the text.
        target: rule.target.clone(),
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
    match (&spec.target, &existing.target) {
        // Both name a subject: same subject means same at-rule, however each
        // was quoted and whatever extra arguments follow.
        (Some(a), Some(b)) => a == b,
        // Neither has one (`@layer base, components;`): fall back to the
        // whitespace-normalised prelude.
        (None, None) => existing.prelude_norm == spec.prelude,
        // One names a subject and the other does not: different rules.
        _ => false,
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
                let Some(at) = crate::locate::at_rule_ref(ctx, &node) else {
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
/// present.
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
/// whose whole prelude) matches `matching` -- see [`AtRuleRef::matches`].
/// `matching` of `None` removes all of them.
pub fn remove_at_rule(
    source: &str,
    name: &str,
    matching: Option<&str>,
    options: ParseOptions,
) -> Result<Outcome> {
    let want_name = name.trim_start_matches('@').to_lowercase();
    let want = matching.map(normalize_target_needle);

    run(source, options, |ctx| {
        let comments = comment_ranges(ctx);
        let mut edits = Vec::new();
        for at in find_top_level_at_rules(ctx) {
            if at.name != want_name || !at.matches(want.as_deref()) {
                continue;
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

/// Render a block body, re-indented to the target but otherwise verbatim.
fn render_block_body(ctx: &ParseCtx, body: &str, indent: &str, single_line: bool) -> String {
    let nl = ctx.nl();
    let content = body.trim_end().trim_start_matches(['\n', '\r']);
    if content.trim().is_empty() {
        return if single_line {
            String::new()
        } else {
            format!("{nl}{indent}")
        };
    }
    if single_line && !content.contains('\n') {
        return format!(" {} ", content.trim());
    }
    let inner_indent = format!("{indent}{}", ctx.indent());
    let inner = reindent(content, &inner_indent, nl);
    format!("{nl}{inner}{nl}{indent}")
}

/// The lowercase at-rule name a caller passed, with or without its `@`.
fn block_name(name: &str) -> Result<String> {
    let name = name.trim().trim_start_matches('@').trim().to_lowercase();
    if name.is_empty() {
        return Err(CssError::InvalidInput("at-rule name is empty".to_string()));
    }
    Ok(name)
}

/// The top-level at-rule `name` a block op works on: the first one, or the
/// first whose target -- or, without one, whose prelude -- is `matching`.
fn find_block_at_rule(ctx: &ParseCtx, name: &str, matching: Option<&str>) -> Option<AtRuleRef> {
    find_at_rules_named(ctx, name)
        .into_iter()
        .find(|at| at.matches(matching))
}

/// The refusal both block ops give an at-rule written as a statement.
fn no_block(name: &str) -> CssError {
    CssError::InvalidInput(format!(
        "@{name} is present without a block; refusing to give it one"
    ))
}

/// Insert `@name matching { declarations }` where an at-rule of this family
/// belongs, its body spliced in verbatim and re-indented.
fn insert_block(
    ctx: &ParseCtx,
    name: &str,
    matching: Option<&str>,
    declarations: &str,
) -> Result<Vec<Edit>> {
    let header = match matching.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => format!("@{name} {m}"),
        None => format!("@{name}"),
    };
    let spec = parse_at_rule_spec(&format!("{header} {{}}"))?;
    let at = insertion_offset(ctx, &spec);
    let nl = ctx.nl();
    let indent = ctx.indent_at(at).to_string();
    let block = format!(
        "{header} {{{}}}",
        render_block_body(ctx, declarations, &indent, false)
    );

    let text = if at == 0 {
        format!("{block}{nl}")
    } else if ctx.source()[..at].ends_with('\n') {
        format!("{indent}{block}{nl}")
    } else {
        format!("{nl}{indent}{block}")
    };
    Ok(vec![Edit::insert(at, text)])
}

/// Give the top-level at-rule `name` this block, replacing an existing body or
/// inserting the whole rule when there is none.
///
/// `matching` narrows to one target the way [`remove_at_rule`] does, and is
/// carried into the inserted prelude.
pub fn ensure_at_rule_block(
    source: &str,
    name: &str,
    matching: Option<&str>,
    declarations: &str,
    options: ParseOptions,
) -> Result<Outcome> {
    let want_name = block_name(name)?;
    validate_snippet(declarations, "declarations")?;
    let want = matching.map(normalize_target_needle);

    run(source, options, |ctx| {
        let Some(at) = find_block_at_rule(ctx, &want_name, want.as_deref()) else {
            return insert_block(ctx, &want_name, matching, declarations);
        };
        let (Some(open), Some(close)) = (at.body_open, at.body_close) else {
            return Err(no_block(&want_name));
        };
        let indent = ctx.indent_at(at.start).to_string();
        let single_line = !ctx.source()[at.start..at.end].contains('\n');
        let replacement = render_block_body(ctx, declarations, &indent, single_line);
        Ok(vec![Edit::replace(open, close, replacement)])
    })
}

/// One declaration a caller asked for, read off the CST of a throwaway rule.
#[derive(Debug, Clone)]
struct Wanted {
    /// [`normalize_property`] of its property, the key it is matched by.
    property: String,
    /// The value as written, its continuation lines relative to the
    /// declaration's own indentation.
    value: String,
    /// [`value_norm`] of the value, what "already set" is decided by.
    value_norm: String,
    important: bool,
    /// The whole declaration, `;`-terminated, at no indentation.
    text: String,
}

/// The declarations of `text`, parsed rather than scanned: a `;` inside a
/// string, a `url()` or a comment ends nothing. A property given twice is
/// given once, at its last value, as CSS reads it.
///
/// Refuses anything that is not a plain list of declarations -- a nested rule
/// or an at-rule has no property to be matched by, and text the parser could
/// not read as a declaration would otherwise be dropped without a word.
fn wanted_declarations(text: &str) -> Result<Vec<Wanted>> {
    let probe = format!("a{{{text}}}");
    let ctx = ParseCtx::try_new(&probe, ParseOptions::default())?;
    let rule = find_top_level_rules(&ctx)
        .into_iter()
        .next()
        .filter(|_| ctx.round_trips())
        .ok_or_else(|| {
            CssError::InvalidInput(format!("cannot understand declarations {text:?}"))
        })?;

    if let Some(item) = non_declaration_items(&ctx, &rule).first() {
        return Err(CssError::InvalidInput(format!(
            "{:?} is not a declaration; pass declarations only",
            ctx.text(item.text_trimmed_range()).trim()
        )));
    }

    let mut wanted: Vec<Wanted> = Vec::new();
    for decl in declarations_in(&ctx, &rule) {
        let wanted_decl = wanted_from(&ctx, &decl);
        match wanted
            .iter_mut()
            .find(|w| w.property == wanted_decl.property)
        {
            Some(earlier) => *earlier = wanted_decl,
            None => wanted.push(wanted_decl),
        }
    }
    Ok(wanted)
}

fn wanted_from(ctx: &ParseCtx, decl: &DeclRef) -> Wanted {
    let src = ctx.source();
    // The whitespace in front of the declaration on its own line, so a value
    // broken over lines keeps its shape relative to the property it belongs to
    // -- whatever precedes that whitespace, the probe's `a{` included.
    let before = &src[ctx.line_start(decl.start)..decl.start];
    let base = before.len() - before.trim_end_matches([' ', '\t']).len();
    let whole = src[decl.start..decl.end].trim_end();
    let whole = if whole.ends_with(';') {
        whole.to_string()
    } else {
        format!("{whole};")
    };

    Wanted {
        property: normalize_property(&decl.property),
        value: relative_lines(src[decl.value_start..decl.value_end].trim(), base),
        value_norm: value_norm(decl),
        important: decl.important,
        text: reindent(&format!("{}{whole}", " ".repeat(base)), "", "\n"),
    }
}

/// `text` with up to `base` columns of indentation taken off every line after
/// the first, which is where a value's continuation lines sit.
fn relative_lines(text: &str, base: usize) -> String {
    let mut lines = text.split('\n');
    let first = lines
        .next()
        .unwrap_or("")
        .trim_end_matches('\r')
        .to_string();
    lines.fold(first, |mut out, line| {
        let line = line.trim_end_matches('\r');
        let own = line.len() - line.trim_start_matches([' ', '\t']).len();
        out.push('\n');
        out.push_str(&line[own.min(base)..]);
        out
    })
}

/// `value` with each continuation line moved under `indent`, the file's own
/// indentation for the declaration it now belongs to.
fn indent_continuation(value: &str, indent: &str, nl: &str) -> String {
    let mut lines = value.split('\n');
    let first = lines.next().unwrap_or("").to_string();
    lines.fold(first, |mut out, line| {
        out.push_str(nl);
        if !line.trim().is_empty() {
            out.push_str(indent);
            out.push_str(line);
        }
        out
    })
}

/// Set each of `declarations` inside the top-level at-rule `name`, and leave
/// everything else in its block as it is -- the other declarations, nested
/// rules and every comment. Insert the whole rule, `declarations` spliced in
/// verbatim, when there is none.
///
/// A property the block already has keeps its place and its comments: only its
/// value bytes are rewritten, and only when the value differs token for token,
/// so whitespace inside a value broken over lines is not a change. A property
/// it does not have is appended after its last item, in the indentation of the
/// items already there. Running it again changes nothing.
///
/// `matching` narrows to one target the way [`remove_at_rule`] does, and is
/// carried into an inserted prelude.
pub fn ensure_at_rule_declarations(
    source: &str,
    name: &str,
    matching: Option<&str>,
    declarations: &str,
    options: ParseOptions,
) -> Result<Outcome> {
    let want_name = block_name(name)?;
    validate_snippet(declarations, "declarations")?;
    let wanted = wanted_declarations(declarations)?;
    let want = matching.map(normalize_target_needle);

    run(source, options, |ctx| {
        let Some(at) = find_block_at_rule(ctx, &want_name, want.as_deref()) else {
            return insert_block(ctx, &want_name, matching, declarations);
        };
        let Some(block) = at_rule_block_rule(&at) else {
            return Err(no_block(&want_name));
        };

        // The last declaration of each property is the one CSS applies, so it
        // is the one a caller means; later entries replace earlier ones here.
        let present: HashMap<String, DeclRef> = declarations_in(ctx, &block)
            .into_iter()
            .map(|d| (normalize_property(&d.property), d))
            .collect();

        let nl = ctx.nl();
        let mut edits = Vec::new();
        let mut missing = Vec::new();
        for w in &wanted {
            let Some(d) = present.get(&w.property) else {
                missing.push(w.text.clone());
                continue;
            };
            if d.important == w.important && value_norm(d) == w.value_norm {
                continue;
            }
            let end = d.important_range.map_or(d.value_end, |(_, end)| end);
            let indent = if ctx.is_at_line_start(d.start) {
                ctx.indent_at(d.start)
            } else {
                ""
            };
            let flag = if w.important { " !important" } else { "" };
            edits.push(Edit::replace(
                d.value_start,
                end,
                format!("{}{flag}", indent_continuation(&w.value, indent, nl)),
            ));
        }

        edits.extend(append_all_to_body(ctx, &block, &missing));
        Ok(edits)
    })
}

/// Remove each of `declarations` from the top-level at-rule `name` -- the
/// inverse of [`ensure_at_rule_declarations`] -- and leave everything else in
/// its block as it is.
///
/// A declaration goes only while it is still the one given: the same property,
/// the same value token for token and the same `!important`. One whose value
/// has been changed since is someone's edit, and stays. A removed declaration
/// takes the comments it owns (see [`crate::trivia`]); a block left with
/// nothing in it takes its at-rule, the way [`remove_at_rule`] removes one.
///
/// `matching` narrows to one target the way [`remove_at_rule`] does. An at-rule
/// that is not there, or has no block, has nothing to remove.
pub fn remove_at_rule_declarations(
    source: &str,
    name: &str,
    matching: Option<&str>,
    declarations: &str,
    options: ParseOptions,
) -> Result<Outcome> {
    let want_name = block_name(name)?;
    validate_snippet(declarations, "declarations")?;
    let wanted: HashMap<String, Wanted> = wanted_declarations(declarations)?
        .into_iter()
        .map(|w| (w.property.clone(), w))
        .collect();
    let want = matching.map(normalize_target_needle);

    run(source, options, |ctx| {
        let Some(at) = find_block_at_rule(ctx, &want_name, want.as_deref()) else {
            return Ok(vec![]);
        };
        let Some(block) = at_rule_block_rule(&at) else {
            return Ok(vec![]);
        };

        let comments = comment_ranges(ctx);
        let spans: Vec<(usize, usize)> = declarations_in(ctx, &block)
            .iter()
            .filter(|d| {
                wanted
                    .get(&normalize_property(&d.property))
                    .is_some_and(|w| d.important == w.important && value_norm(d) == w.value_norm)
            })
            .map(|d| {
                let span = deletion_span(ctx, &comments, d.start, d.end);
                // Sharing a line with another item: the space it was set off by
                // goes too, so `{ --a: 1; --b: 2; }` keeps one between the rest.
                let end = if ctx.is_at_line_start(d.start) {
                    span.end
                } else {
                    span.end + spaces_at(ctx.source(), span.end)
                };
                (span.start, end)
            })
            .collect();
        if spans.is_empty() {
            return Ok(vec![]);
        }

        if only_whitespace_left(ctx.source(), block.body_open, block.body_close, &spans) {
            let span = deletion_span(ctx, &comments, at.start, at.end);
            let span = absorb_surrounding_blank_line(ctx, span);
            return Ok(vec![Edit::delete(span.start, span.end)]);
        }
        Ok(spans
            .into_iter()
            .map(|(start, end)| Edit::delete(start, end))
            .collect())
    })
}

/// How many spaces and tabs `src` has from `at` on.
fn spaces_at(src: &str, at: usize) -> usize {
    src[at..].len() - src[at..].trim_start_matches([' ', '\t']).len()
}

/// Is `[open, close)` whitespace once `spans` -- inside it, in source order --
/// are taken out?
fn only_whitespace_left(src: &str, open: usize, close: usize, spans: &[(usize, usize)]) -> bool {
    let mut at = open;
    for &(start, end) in spans {
        if !src[at..start.max(at)].trim().is_empty() {
            return false;
        }
        at = end.max(at);
    }
    src[at.min(close)..close].trim().is_empty()
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

    fn ensure_block(src: &str, name: &str, matching: Option<&str>, decls: &str) -> Outcome {
        ensure_at_rule_block(src, name, matching, decls, ParseOptions::default()).unwrap()
    }

    // -- block at-rules -----------------------------------------------------

    #[test]
    fn inserts_a_block_at_rule_when_absent() {
        let out = ensure_block(
            "@import \"tailwindcss\";\n",
            "theme",
            None,
            "--color-a: red;",
        );
        assert!(out.changed);
        assert_eq!(
            out.source,
            "@import \"tailwindcss\";\n@theme {\n  --color-a: red;\n}\n"
        );
    }

    #[test]
    fn replaces_the_body_of_an_existing_block_at_rule() {
        let src = "@theme {\n  --color-a: red;\n}\n";
        let out = ensure_block(src, "theme", None, "--color-b: blue;");
        assert!(out.changed);
        assert_eq!(out.source, "@theme {\n  --color-b: blue;\n}\n");
    }

    #[test]
    fn replacing_a_block_at_rule_with_the_same_body_is_a_no_op() {
        let src = "@theme {\n  --color-a: red;\n}\n";
        let out = ensure_block(src, "theme", None, "--color-a: red;");
        assert!(!out.changed);
        assert_eq!(out.source, src);
    }

    #[test]
    fn keeps_a_single_line_block_on_one_line() {
        let out = ensure_block(
            "@theme { --color-a: red; }\n",
            "theme",
            None,
            "--color-b: blue;",
        );
        assert_eq!(out.source, "@theme { --color-b: blue; }\n");
    }

    #[test]
    fn accepts_the_name_with_or_without_the_at_sign() {
        let a = ensure_block("", "theme", None, "--a: 1;");
        let b = ensure_block("", "@theme", None, "--a: 1;");
        assert_eq!(a.source, b.source);
    }

    #[test]
    fn narrows_to_a_matching_target() {
        let src = "@plugin \"a\" {\n  x: 1;\n}\n@plugin \"b\" {\n  y: 2;\n}\n";
        let out = ensure_block(src, "plugin", Some("\"b\""), "y: 3;");
        assert_eq!(
            out.source,
            "@plugin \"a\" {\n  x: 1;\n}\n@plugin \"b\" {\n  y: 3;\n}\n"
        );
    }

    #[test]
    fn carries_matching_into_an_inserted_prelude() {
        let out = ensure_block("", "plugin", Some("\"daisyui\""), "prefix: \"d-\";");
        assert_eq!(out.source, "@plugin \"daisyui\" {\n  prefix: \"d-\";\n}\n");
    }

    #[test]
    fn empties_a_block_given_no_declarations() {
        let out = ensure_block("@theme {\n  --a: 1;\n}\n", "theme", None, "");
        assert_eq!(out.source, "@theme {\n}\n");
    }

    #[test]
    fn refuses_to_give_a_block_to_a_statement_at_rule() {
        let err = ensure_at_rule_block(
            "@import \"a.css\";\n",
            "import",
            None,
            "x: 1;",
            ParseOptions::default(),
        );
        assert!(err.is_err());
    }

    #[test]
    fn keeps_comments_and_grouping_in_a_spliced_body() {
        let body = "  --a: 1;\n\n  /* group */\n  --b: 2;";
        let out = ensure_block("", "theme", None, body);
        assert_eq!(
            out.source,
            "@theme {\n  --a: 1;\n\n  /* group */\n  --b: 2;\n}\n"
        );
    }

    #[test]
    fn reindents_a_body_taken_from_another_file() {
        let body = "        --a: 1;\n        --b: 2;";
        let out = ensure_block("", "theme", None, body);
        assert_eq!(out.source, "@theme {\n  --a: 1;\n  --b: 2;\n}\n");
    }

    #[test]
    fn rejects_an_empty_name() {
        let err = ensure_at_rule_block("", "@", None, "x: 1;", ParseOptions::default());
        assert!(err.is_err());
    }

    #[test]
    fn rejects_declarations_that_would_unbalance_the_file() {
        let err = ensure_at_rule_block("", "theme", None, "x: 1; }", ParseOptions::default());
        assert!(err.is_err());
    }

    // -- declarations inside a block at-rule ----------------------------------

    fn ensure_decls(src: &str, name: &str, matching: Option<&str>, decls: &str) -> Outcome {
        ensure_at_rule_declarations(src, name, matching, decls, ParseOptions::default()).unwrap()
    }

    const APP_THEME: &str = "@import \"tailwindcss\";\n\n@theme {\n    /* Custom font family */\n    --font-caveat: \"caveat\", cursive;\n\n    /* Brand */\n    --color-brand: #1eb0ff;\n    --color-base-border-light: red;\n}\n";

    #[test]
    fn sets_its_declarations_and_keeps_every_other_line_of_the_block() {
        let out = ensure_decls(
            APP_THEME,
            "theme",
            None,
            "--color-base-border-light: var(--base-border-light);\n--color-primary-light: var(--primary-light);",
        );
        assert!(out.changed);
        assert_eq!(
            out.source,
            "@import \"tailwindcss\";\n\n@theme {\n    /* Custom font family */\n    --font-caveat: \"caveat\", cursive;\n\n    /* Brand */\n    --color-brand: #1eb0ff;\n    --color-base-border-light: var(--base-border-light);\n    --color-primary-light: var(--primary-light);\n}\n"
        );
    }

    #[test]
    fn running_it_again_changes_nothing() {
        let decls = "--color-base-border-light: var(--base-border-light);\n--color-primary-light: var(--primary-light);";
        let once = ensure_decls(APP_THEME, "theme", None, decls);
        let twice = ensure_decls(&once.source, "theme", None, decls);
        assert!(!twice.changed);
        assert_eq!(twice.source, once.source);
    }

    #[test]
    fn a_declaration_already_set_is_left_alone() {
        let out = ensure_decls(APP_THEME, "theme", None, "--color-brand: #1eb0ff;");
        assert!(!out.changed);
        assert_eq!(out.source, APP_THEME);
    }

    #[test]
    fn rewrites_only_the_value_bytes_so_an_inline_comment_survives() {
        let src = "@theme {\n  --a: 1; /* keep me */\n  --b: 2;\n}\n";
        let out = ensure_decls(src, "theme", None, "--a: 9;");
        assert_eq!(
            out.source,
            "@theme {\n  --a: 9; /* keep me */\n  --b: 2;\n}\n"
        );
    }

    #[test]
    fn appends_every_missing_declaration_in_order_as_one_edit() {
        let src = "@theme {\n  --a: 1;\n}\n";
        let out = ensure_decls(src, "theme", None, "--b: 2; --c: 3; --d: 4;");
        assert_eq!(
            out.source,
            "@theme {\n  --a: 1;\n  --b: 2;\n  --c: 3;\n  --d: 4;\n}\n"
        );
    }

    #[test]
    fn whitespace_inside_a_value_broken_over_lines_is_not_a_change() {
        let src = "@theme {\n  --g: var(\n      --x\n    );\n}\n";
        let out = ensure_decls(src, "theme", None, "--g: var(--x);");
        assert!(!out.changed);
    }

    #[test]
    fn a_multi_line_value_keeps_its_shape_when_appended() {
        let src = "@theme {\n    --a: 1;\n}\n";
        let decls = "  --g: linear-gradient(\n    to right,\n    red\n  );";
        let out = ensure_decls(src, "theme", None, decls);
        assert_eq!(
            out.source,
            "@theme {\n    --a: 1;\n    --g: linear-gradient(\n      to right,\n      red\n    );\n}\n"
        );
        assert!(!ensure_decls(&out.source, "theme", None, decls).changed);
    }

    #[test]
    fn a_multi_line_value_keeps_its_shape_when_it_replaces_one() {
        let src = "@theme {\n    --g: red;\n}\n";
        let decls = "  --g: linear-gradient(\n    to right,\n    red\n  );";
        let out = ensure_decls(src, "theme", None, decls);
        assert_eq!(
            out.source,
            "@theme {\n    --g: linear-gradient(\n      to right,\n      red\n    );\n}\n"
        );
        assert!(!ensure_decls(&out.source, "theme", None, decls).changed);
    }

    #[test]
    fn inserts_the_whole_block_verbatim_when_there_is_none() {
        let out = ensure_decls(
            "@import \"tailwindcss\";\n",
            "theme",
            None,
            "--a: 1;\n\n/* group */\n--b: 2;",
        );
        assert_eq!(
            out.source,
            "@import \"tailwindcss\";\n@theme {\n  --a: 1;\n\n  /* group */\n  --b: 2;\n}\n"
        );
    }

    #[test]
    fn narrows_to_a_matching_target_and_leaves_its_siblings_alone() {
        let src = "@plugin \"a\" {\n  x: 1;\n}\n@plugin \"b\" {\n  x: 1;\n}\n";
        let out = ensure_decls(src, "plugin", Some("\"b\""), "x: 2; y: 3;");
        assert_eq!(
            out.source,
            "@plugin \"a\" {\n  x: 1;\n}\n@plugin \"b\" {\n  x: 2;\n  y: 3;\n}\n"
        );
    }

    #[test]
    fn keeps_a_nested_block_whole_and_appends_after_it() {
        let src =
            "@theme {\n  --animate-w: w 1s;\n  @keyframes w {\n    50% { opacity: 0; }\n  }\n}\n";
        let out = ensure_decls(src, "theme", None, "--a: 1;");
        assert_eq!(
            out.source,
            "@theme {\n  --animate-w: w 1s;\n  @keyframes w {\n    50% { opacity: 0; }\n  }\n  --a: 1;\n}\n"
        );
    }

    #[test]
    fn a_semicolon_inside_a_value_ends_nothing() {
        let src = "@theme {\n  --a: 1;\n}\n";
        let decls = "--bg: url(\"data:image/svg+xml;utf8,<svg/>\"); --c: \"a;b\";";
        let out = ensure_decls(src, "theme", None, decls);
        assert_eq!(
            out.source,
            "@theme {\n  --a: 1;\n  --bg: url(\"data:image/svg+xml;utf8,<svg/>\");\n  --c: \"a;b\";\n}\n"
        );
    }

    #[test]
    fn a_property_given_twice_is_set_once_at_its_last_value() {
        let out = ensure_decls("@theme {\n  --a: 1;\n}\n", "theme", None, "--b: 1; --b: 2;");
        assert_eq!(out.source, "@theme {\n  --a: 1;\n  --b: 2;\n}\n");
    }

    #[test]
    fn the_last_of_a_property_the_block_repeats_is_the_one_set() {
        let src = "@theme {\n  --a: 1;\n  --a: 2;\n}\n";
        let out = ensure_decls(src, "theme", None, "--a: 3;");
        assert_eq!(out.source, "@theme {\n  --a: 1;\n  --a: 3;\n}\n");
    }

    #[test]
    fn custom_properties_are_matched_case_sensitively() {
        let src = "@theme {\n  --Brand: red;\n}\n";
        let out = ensure_decls(src, "theme", None, "--brand: blue;");
        assert_eq!(
            out.source,
            "@theme {\n  --Brand: red;\n  --brand: blue;\n}\n"
        );
    }

    #[test]
    fn sets_and_clears_the_important_flag_as_written() {
        let src = "@plugin \"p\" {\n  a: 1;\n  b: 2 !important;\n}\n";
        let out = ensure_decls(src, "plugin", Some("p"), "a: 1 !important; b: 2;");
        assert_eq!(
            out.source,
            "@plugin \"p\" {\n  a: 1 !important;\n  b: 2;\n}\n"
        );
    }

    #[test]
    fn declarations_keep_a_single_line_block_on_one_line() {
        let out = ensure_decls("@theme { --a: 1; }\n", "theme", None, "--b: 2;");
        assert_eq!(out.source, "@theme { --a: 1; --b: 2; }\n");
    }

    #[test]
    fn fills_an_empty_block() {
        let out = ensure_decls("@theme {\n}\n", "theme", None, "--a: 1; --b: 2;");
        assert_eq!(out.source, "@theme {\n  --a: 1;\n  --b: 2;\n}\n");
    }

    #[test]
    fn declarations_use_the_files_newline_style() {
        let src = "@theme {\r\n  --a: 1;\r\n}\r\n";
        let out = ensure_decls(src, "theme", None, "--b: 2; --c: 3;");
        assert_eq!(
            out.source,
            "@theme {\r\n  --a: 1;\r\n  --b: 2;\r\n  --c: 3;\r\n}\r\n"
        );
    }

    #[test]
    fn terminates_a_last_declaration_written_without_a_semicolon() {
        let out = ensure_decls("@theme {\n  --a: 1\n}\n", "theme", None, "--b: 2;");
        assert_eq!(out.source, "@theme {\n  --a: 1;\n  --b: 2;\n}\n");
    }

    #[test]
    fn refuses_what_is_not_a_plain_list_of_declarations() {
        for decls in [
            ".x { color: red; }",
            "@media print { a: 1; }",
            "color red;",
            "--a: 1; }",
        ] {
            assert!(
                ensure_at_rule_declarations(
                    "@theme {\n}\n",
                    "theme",
                    None,
                    decls,
                    ParseOptions::default()
                )
                .is_err(),
                "accepted {decls:?}"
            );
        }
    }

    #[test]
    fn refuses_to_give_declarations_to_a_statement_at_rule() {
        let err = ensure_at_rule_declarations(
            "@source \"../js\";\n",
            "source",
            None,
            "x: 1;",
            ParseOptions::default(),
        );
        assert!(err.is_err());
    }

    #[test]
    fn declarations_reject_an_empty_name() {
        assert!(
            ensure_at_rule_declarations("", "@", None, "x: 1;", ParseOptions::default()).is_err()
        );
    }

    #[test]
    fn leaves_the_rest_of_the_stylesheet_byte_for_byte() {
        let src =
            "/* head */\n@import \"a\";\n@theme {\n  --a: 1;\n}\n.btn { color: red; } /* tail */\n";
        let out = ensure_decls(src, "theme", None, "--a: 2;");
        assert_eq!(out.source, src.replace("--a: 1;", "--a: 2;"));
    }

    // -- removing declarations from a block at-rule --------------------------

    fn remove_decls(src: &str, name: &str, matching: Option<&str>, decls: &str) -> Outcome {
        remove_at_rule_declarations(src, name, matching, decls, ParseOptions::default()).unwrap()
    }

    const PROJECT_THEME: &str = "@import \"tailwindcss\";\n\n@theme {\n    /* Custom font family */\n    --font-caveat: \"caveat\", cursive;\n\n    /* Brand */\n    --color-brand: #1eb0ff;\n}\n";
    const LIBRARY_TOKENS: &str =
        "--color-a-light: var(--a-light);\n--color-b-light: var(\n    --b-light\n);";

    #[test]
    fn removing_what_was_set_gives_the_projects_block_back() {
        let set = ensure_decls(PROJECT_THEME, "theme", None, LIBRARY_TOKENS);
        assert!(set.source.contains("--color-b-light"));
        let out = remove_decls(&set.source, "theme", None, LIBRARY_TOKENS);
        assert!(out.changed);
        assert_eq!(out.source, PROJECT_THEME);
    }

    #[test]
    fn removing_what_was_inserted_gives_the_file_back() {
        for src in [
            "@import \"tailwindcss\";\n",
            "@import \"tailwindcss\";\n\n.btn {\n  color: red;\n}\n",
            "@import \"tailwindcss\";\n@plugin \"x\";\n\n@layer base {\n  a { color: red; }\n}\n",
            ".btn { color: red; }\n",
            "",
        ] {
            let set = ensure_decls(src, "theme", Some(""), LIBRARY_TOKENS);
            assert!(set.source.contains("@theme {"), "{src:?}");
            let out = remove_decls(&set.source, "theme", Some(""), LIBRARY_TOKENS);
            assert_eq!(out.source, src, "{src:?}");
        }
    }

    #[test]
    fn keeps_a_declaration_whose_value_was_changed() {
        let src = "@theme {\n  --a: red;\n  --b: 2;\n}\n";
        let out = remove_decls(src, "theme", None, "--a: var(--a); --b: 2;");
        assert_eq!(out.source, "@theme {\n  --a: red;\n}\n");
    }

    #[test]
    fn compares_values_token_for_token() {
        let src = "@theme {\n  --keep: 1;\n  --a: var(\n      --a\n  );\n}\n";
        let out = remove_decls(src, "theme", None, "--a: var(--a);");
        assert_eq!(out.source, "@theme {\n  --keep: 1;\n}\n");
    }

    #[test]
    fn important_must_match_too() {
        let src = "@theme {\n  --a: 1 !important;\n  --b: 2;\n}\n";
        assert!(!remove_decls(src, "theme", None, "--a: 1;").changed);
        let out = remove_decls(src, "theme", None, "--a: 1 !important;");
        assert_eq!(out.source, "@theme {\n  --b: 2;\n}\n");
    }

    #[test]
    fn a_removed_declaration_takes_the_comments_it_owns() {
        let src = "@theme {\n  /* ===== tokens ===== */\n  --keep: 1;\n\n  /* about the block */\n\n  /* ours */\n  --a: 1; /* trailing */\n}\n";
        let out = remove_decls(src, "theme", None, "--a: 1;");
        assert_eq!(
            out.source,
            "@theme {\n  /* ===== tokens ===== */\n  --keep: 1;\n\n  /* about the block */\n\n}\n"
        );
    }

    #[test]
    fn a_block_with_a_comment_left_stays() {
        let src = "@theme {\n  /* tokens */\n\n  --a: 1;\n}\n";
        let out = remove_decls(src, "theme", None, "--a: 1;");
        assert_eq!(out.source, "@theme {\n  /* tokens */\n\n}\n");
    }

    #[test]
    fn a_block_left_empty_goes_with_the_comments_it_owns() {
        let src = "@import \"x\";\n\n/* library tokens */\n@theme {\n  --a: 1;\n  /* b */\n  --b: 2;\n}\n\n.btn { color: red; }\n";
        let out = remove_decls(src, "theme", None, "--a: 1; --b: 2;");
        assert_eq!(out.source, "@import \"x\";\n\n.btn { color: red; }\n");
    }

    #[test]
    fn a_block_with_a_nested_rule_left_stays() {
        let src = "@theme {\n  --a: 1;\n  @keyframes spin {\n    to { rotate: 360deg; }\n  }\n}\n";
        let out = remove_decls(src, "theme", None, "--a: 1;");
        assert_eq!(
            out.source,
            "@theme {\n  @keyframes spin {\n    to { rotate: 360deg; }\n  }\n}\n"
        );
    }

    #[test]
    fn removes_every_copy_that_is_still_the_one_given() {
        let src = "@theme {\n  --a: 1;\n  --a: 2;\n  --a: 1;\n}\n";
        let out = remove_decls(src, "theme", None, "--a: 1;");
        assert_eq!(out.source, "@theme {\n  --a: 2;\n}\n");
    }

    #[test]
    fn a_single_line_block_keeps_one_space_between_what_is_left() {
        let src = "@theme { --a: 1; --b: 2; --c: 3; }\n";
        assert_eq!(
            remove_decls(src, "theme", None, "--a: 1;").source,
            "@theme { --b: 2; --c: 3; }\n"
        );
        assert_eq!(
            remove_decls(src, "theme", None, "--b: 2;").source,
            "@theme { --a: 1; --c: 3; }\n"
        );
        assert_eq!(
            remove_decls(src, "theme", None, "--c: 3;").source,
            "@theme { --a: 1; --b: 2; }\n"
        );
        assert_eq!(
            remove_decls(src, "theme", None, "--a: 1; --b: 2; --c: 3;").source,
            ""
        );
    }

    #[test]
    fn removing_uses_the_files_newline_style() {
        let src = "@theme {\r\n  --keep: 1;\r\n  --a: 1;\r\n}\r\n";
        let out = remove_decls(src, "theme", None, "--a: 1;");
        assert_eq!(out.source, "@theme {\r\n  --keep: 1;\r\n}\r\n");
    }

    #[test]
    fn matching_names_the_block_to_remove_from() {
        let src = "@theme inline {\n  --a: 1;\n}\n\n@theme {\n  --a: 1;\n}\n";
        assert_eq!(
            remove_decls(src, "theme", Some(""), "--a: 1;").source,
            "@theme inline {\n  --a: 1;\n}\n"
        );
        assert_eq!(
            remove_decls(src, "theme", Some("inline"), "--a: 1;").source,
            "@theme {\n  --a: 1;\n}\n"
        );
    }

    #[test]
    fn nothing_to_remove_is_no_change() {
        for (src, decls) in [
            ("@import \"x\";\n", "--a: 1;"),
            ("@theme {\n  --b: 2;\n}\n", "--a: 1;"),
            ("@theme {\n}\n", "--a: 1;"),
            ("@source \"../js\";\n", "--a: 1;"),
        ] {
            let name = if src.starts_with("@source") {
                "source"
            } else {
                "theme"
            };
            let out = remove_decls(src, name, None, decls);
            assert!(!out.changed, "{src:?}");
            assert_eq!(out.source, src);
        }
    }

    #[test]
    fn removing_twice_is_removing_once() {
        let set = ensure_decls(PROJECT_THEME, "theme", None, LIBRARY_TOKENS);
        let once = remove_decls(&set.source, "theme", None, LIBRARY_TOKENS);
        let twice = remove_decls(&once.source, "theme", None, LIBRARY_TOKENS);
        assert!(!twice.changed);
        assert_eq!(twice.source, once.source);
    }

    #[test]
    fn removing_leaves_the_rest_of_the_stylesheet_byte_for_byte() {
        let src = "/* head */\n@import \"a\";\n@theme {\n  --keep: 1;\n  --a: 1;\n}\n.btn { color: red; } /* tail */\n";
        let out = remove_decls(src, "theme", None, "--a: 1;");
        assert_eq!(out.source, src.replace("  --a: 1;\n", ""));
    }

    #[test]
    fn removing_refuses_what_is_not_a_plain_list_of_declarations() {
        for decls in [
            ".x { color: red; }",
            "@media print { a: 1; }",
            "color red;",
            "--a: 1; }",
        ] {
            assert!(
                remove_at_rule_declarations(
                    "@theme {\n  --a: 1;\n}\n",
                    "theme",
                    None,
                    decls,
                    ParseOptions::default()
                )
                .is_err(),
                "accepted {decls:?}"
            );
        }
        assert!(
            remove_at_rule_declarations("", "@", None, "x: 1;", ParseOptions::default()).is_err()
        );
    }

    #[test]
    fn an_empty_matching_is_an_at_rule_with_nothing_after_its_name() {
        let src = "@theme inline {\n  --font: x;\n}\n";
        let out = ensure_decls(src, "theme", Some(""), "--a: 1;");
        assert!(out.source.starts_with(src), "{:?}", out.source);
        assert!(
            out.source.contains("@theme {\n  --a: 1;\n}"),
            "{:?}",
            out.source
        );
        let again = ensure_decls(&out.source, "theme", Some(""), "--a: 1;");
        assert!(!again.changed);
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
    fn removing_the_last_at_rule_leaves_no_blank_line_at_the_end() {
        let o = remove("@import \"a\";\n\n@plugin \"x\";\n", "plugin", None);
        assert_eq!(o.source, "@import \"a\";\n");
    }

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
    fn targets_are_read_from_the_cst_not_scanned_from_text() {
        use crate::ctx::ParseCtx;
        use crate::locate::find_top_level_at_rules;

        let cases = [
            (r#"@import "a/b.css";"#, Some("a/b.css")),
            (r#"@import 'a';"#, Some("a")),
            (r#"@import url("x");"#, Some("x")),
            ("@import url(x);", Some("x")),
            ("@layer base, components;", None),
            // A quote inside a comment must not be mistaken for the target.
            (r#"@import /* "decoy" */ "real.css";"#, Some("real.css")),
            // A block at-rule still has a subject when one precedes the `{`,
            // so `@plugin "p" { ... }` dedupes against `@plugin "p";`.
            (r#"@plugin "p" { name: "decoy"; }"#, Some("p")),
            // But a string that only appears *inside* the block is not it.
            (r#"@theme { --font: "decoy"; }"#, None),
        ];

        for (src, expected) in cases {
            let ctx = ParseCtx::parse_default(src);
            let rules = find_top_level_at_rules(&ctx);
            assert_eq!(
                rules[0].target.as_deref(),
                expected,
                "wrong target for {src}"
            );
        }
    }

    #[test]
    fn a_caller_supplied_needle_is_unquoted() {
        assert_eq!(normalize_target_needle("a.css"), "a.css");
        assert_eq!(normalize_target_needle("\"a.css\""), "a.css");
        assert_eq!(normalize_target_needle("url(\"a.css\")"), "a.css");
        assert_eq!(normalize_target_needle("url(a.css)"), "a.css");
    }
}
