// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Phase 2: typed queries over the CST that return **byte ranges**, never owned
//! strings to be reprinted.
//!
//! Matching rules for v1 are deliberately strict (ROADMAP §8 Phase 2):
//!   * top-level rules only, unless the caller explicitly opts into descending;
//!   * selectors compared on a normalised form, never raw equality and never
//!     substring/fuzzy;
//!   * more than one match is `MatchResult::Ambiguous` -- we error, we do not
//!     pick one. Guessing here is how the Python version produced surprising
//!     diffs (R4).

use crate::ctx::{is_bogus, ParseCtx};
use biome_css_syntax::{CssSyntaxKind, CssSyntaxNode};
use biome_rowan::{Direction, TextRange};

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

/// A top-level (or scoped) qualified rule: `selector { ... }`.
#[derive(Debug, Clone)]
pub struct RuleRef {
    pub node: CssSyntaxNode,
    /// Selector text exactly as the user wrote it.
    pub selector_raw: String,
    /// Selector text after `normalize_selector`, used for comparisons.
    pub selector_norm: String,
    /// Start of the rule's own bytes (leading trivia excluded).
    pub start: usize,
    /// End of the rule's own bytes (trailing trivia excluded).
    pub end: usize,
    /// Byte offset just after the opening `{`.
    pub body_open: usize,
    /// Byte offset of the closing `}`.
    pub body_close: usize,
}

impl RuleRef {
    /// True when the body contains no declarations or nested rules.
    pub fn body_is_empty(&self, ctx: &ParseCtx) -> bool {
        ctx.source()[self.body_open..self.body_close]
            .trim()
            .is_empty()
    }
}

/// An at-rule: `@name prelude;` or `@name prelude { ... }`.
#[derive(Debug, Clone)]
pub struct AtRuleRef {
    pub node: CssSyntaxNode,
    /// Lowercased at-rule name without the `@`, e.g. `import`, `plugin`, `media`.
    pub name: String,
    /// Everything between the name and the `;` or `{`, trimmed.
    pub prelude: String,
    /// The at-rule's subject, read from the CST: the first string literal or
    /// `url()` value appearing before any block, unquoted.
    ///
    /// Token-derived rather than scanned out of `prelude`, so a quote inside a
    /// comment or a later argument cannot be mistaken for the target.
    pub target: Option<String>,
    pub start: usize,
    pub end: usize,
    pub has_block: bool,
    /// Present only when `has_block`.
    pub body_open: Option<usize>,
    pub body_close: Option<usize>,
}

/// A declaration: `property: value;`.
#[derive(Debug, Clone)]
pub struct DeclRef {
    /// The `CSS_DECLARATION_WITH_SEMICOLON` (or bare `CSS_DECLARATION`) node.
    pub node: CssSyntaxNode,
    pub property: String,
    pub value_raw: String,
    pub important: bool,
    /// Start of the declaration's own bytes.
    pub start: usize,
    /// End of the declaration's own bytes, semicolon included when present.
    pub end: usize,
    /// Range of the value alone -- the only bytes `set_declaration` replaces
    /// when the property already exists (rule E).
    pub value_start: usize,
    pub value_end: usize,
    /// Range of the `!important` flag, when present.
    pub important_range: Option<(usize, usize)>,
}

#[derive(Debug, Clone)]
pub enum MatchResult {
    None,
    One(Box<RuleRef>),
    /// More than one top-level rule matched. Callers must error out.
    Ambiguous(Vec<RuleRef>),
}

impl MatchResult {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn one(self) -> Option<RuleRef> {
        match self {
            Self::One(r) => Some(*r),
            _ => None,
        }
    }

    pub fn count(&self) -> usize {
        match self {
            Self::None => 0,
            Self::One(_) => 1,
            Self::Ambiguous(v) => v.len(),
        }
    }
}

// ---------------------------------------------------------------------------
// Small syntax helpers (the only place that knows Biome's shape)
// ---------------------------------------------------------------------------

fn range_to_pair(range: TextRange) -> (usize, usize) {
    (usize::from(range.start()), usize::from(range.end()))
}

/// Trimmed byte range of a node -- its own text, without leading or trailing
/// trivia. `text_range()` would include the comment sitting above it.
fn trimmed(node: &CssSyntaxNode) -> (usize, usize) {
    range_to_pair(node.text_trimmed_range())
}

/// A child node that opens with `{`. Kind-agnostic on purpose: CSS has a dozen
/// block kinds and new ones appear between Biome releases (R2).
fn block_child(node: &CssSyntaxNode) -> Option<CssSyntaxNode> {
    node.children().find(|c| {
        c.first_token()
            .is_some_and(|t| t.kind() == CssSyntaxKind::L_CURLY)
    })
}

fn block_bounds(block: &CssSyntaxNode) -> Option<(usize, usize)> {
    let open = block
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == CssSyntaxKind::L_CURLY)?;
    let close = block
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .filter(|t| t.kind() == CssSyntaxKind::R_CURLY)
        .last();
    // Trimmed, so the body starts immediately after `{` rather than after the
    // brace's trailing whitespace. Callers rebuilding a body need that space to
    // be part of the range they replace.
    let open_end = usize::from(open.text_trimmed_range().end());
    let close_start = match close {
        // `text_trimmed_range().start()` skips the newline+indent trivia that
        // precedes the closing brace, which is what we want as the body end.
        Some(t) => usize::from(t.text_trimmed_range().start()),
        // Unbalanced input: the block runs to the end of its own text.
        None => usize::from(block.text_trimmed_range().end()),
    };
    Some((open_end, close_start.max(open_end)))
}

/// The list node inside a block that holds declarations and nested rules.
fn block_items(block: &CssSyntaxNode) -> Vec<CssSyntaxNode> {
    block
        .children()
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

fn is_declaration_item(kind: CssSyntaxKind) -> bool {
    matches!(
        kind,
        CssSyntaxKind::CSS_DECLARATION_WITH_SEMICOLON | CssSyntaxKind::CSS_DECLARATION
    )
}

// ---------------------------------------------------------------------------
// Selector normalisation
// ---------------------------------------------------------------------------

/// Canonical form of a selector for comparison purposes.
///
/// Collapses whitespace runs, puts exactly one space around the `>`, `+`, `~`
/// combinators, and exactly one space after each `,`. Text inside quotes is
/// copied verbatim; text inside `[...]` keeps its own spacing rules so that
/// `[a~="b"]` is not mangled into something unrecognisable.
///
/// This does not need to be semantically perfect -- it needs to be
/// *deterministic*, so that the same selector written two ways lands on the
/// same string and two different selectors do not collide.
pub fn normalize_selector(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut pending_space = false;

    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' => {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                let quote = c;
                out.push(quote);
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
            c if c.is_whitespace() => {
                pending_space = true;
            }
            '>' | '+' | '~' if bracket_depth == 0 && paren_depth == 0 => {
                // Combinator: exactly one space on each side.
                while out.ends_with(' ') {
                    out.pop();
                }
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push(c);
                out.push(' ');
                pending_space = false;
            }
            ',' => {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push(',');
                out.push(' ');
                pending_space = false;
            }
            _ => {
                if pending_space && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                pending_space = false;
                match c {
                    '[' => bracket_depth += 1,
                    ']' => bracket_depth = bracket_depth.saturating_sub(1),
                    '(' => paren_depth += 1,
                    ')' => paren_depth = paren_depth.saturating_sub(1),
                    _ => {}
                }
                out.push(c);
            }
        }
    }

    out.trim().to_string()
}

/// Canonical form of a property name: lowercased, trimmed. Custom properties
/// (`--foo`) are case-sensitive per spec, so those keep their case.
pub fn normalize_property(input: &str) -> String {
    let t = input.trim();
    if t.starts_with("--") {
        t.to_string()
    } else {
        t.to_lowercase()
    }
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

fn rule_ref_from(node: &CssSyntaxNode, ctx: &ParseCtx) -> Option<RuleRef> {
    if node.kind() != CssSyntaxKind::CSS_QUALIFIED_RULE {
        return None;
    }
    let selector_list = node
        .children()
        .find(|c| c.kind() == CssSyntaxKind::CSS_SELECTOR_LIST)?;
    let block = block_child(node)?;
    let (body_open, body_close) = block_bounds(&block)?;
    let (start, end) = trimmed(node);
    let selector_raw = ctx.text(selector_list.text_trimmed_range()).to_string();

    Some(RuleRef {
        selector_norm: normalize_selector(&selector_raw),
        selector_raw,
        node: node.clone(),
        start,
        end,
        body_open,
        body_close,
    })
}

/// Strip one layer of matching quotes from a string literal's text.
pub fn unquote(text: &str) -> String {
    let t = text.trim();
    let bytes = t.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        if (first == b'"' || first == b'\'') && bytes[bytes.len() - 1] == first {
            return t[1..t.len() - 1].to_string();
        }
    }
    t.to_string()
}

/// The first string-literal or raw-url token inside an at-rule, ignoring
/// anything inside its block.
fn at_rule_target_token(node: &CssSyntaxNode) -> Option<String> {
    for token in node.descendants_tokens(Direction::Next) {
        match token.kind() {
            // A block starts here; its contents are not the at-rule's subject.
            CssSyntaxKind::L_CURLY => return None,
            CssSyntaxKind::CSS_STRING_LITERAL => return Some(unquote(token.text_trimmed())),
            CssSyntaxKind::CSS_URL_VALUE_RAW_LITERAL => {
                return Some(token.text_trimmed().trim().to_string())
            }
            _ => {}
        }
    }
    None
}

fn at_rule_ref_from(node: &CssSyntaxNode, ctx: &ParseCtx) -> Option<AtRuleRef> {
    if node.kind() != CssSyntaxKind::CSS_AT_RULE {
        return None;
    }
    // `CSS_AT_RULE` is `AT` + one inner node that carries the name and body.
    let inner = node.children().next()?;
    let name_token = inner.first_token()?;
    let name = name_token.text_trimmed().trim().to_lowercase();

    let (start, end) = trimmed(node);
    let name_end = usize::from(name_token.text_trimmed_range().end());

    let block = block_child(&inner);
    let has_block = block.is_some();
    let (body_open, body_close) = match &block {
        Some(b) => {
            let (o, c) = block_bounds(b)?;
            (Some(o), Some(c))
        }
        None => (None, None),
    };

    // Prelude runs from just past the name to the `{` or the trailing `;`.
    let prelude_end = match &block {
        Some(b) => usize::from(b.text_trimmed_range().start()),
        None => {
            let semi = inner
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .filter(|t| t.kind() == CssSyntaxKind::SEMICOLON)
                .last();
            match semi {
                Some(t) => usize::from(t.text_trimmed_range().start()),
                None => end,
            }
        }
    };
    let prelude = ctx
        .source()
        .get(name_end..prelude_end.max(name_end))
        .unwrap_or("")
        .trim()
        .to_string();

    Some(AtRuleRef {
        target: at_rule_target_token(node),
        node: node.clone(),
        name,
        prelude,
        start,
        end,
        has_block,
        body_open,
        body_close,
    })
}

fn decl_ref_from(node: &CssSyntaxNode, ctx: &ParseCtx) -> Option<DeclRef> {
    let (item, decl) = match node.kind() {
        CssSyntaxKind::CSS_DECLARATION_WITH_SEMICOLON => {
            let d = node
                .children()
                .find(|c| c.kind() == CssSyntaxKind::CSS_DECLARATION)?;
            (node.clone(), d)
        }
        CssSyntaxKind::CSS_DECLARATION => (node.clone(), node.clone()),
        _ => return None,
    };

    let property_node = decl.children().find(|c| {
        matches!(
            c.kind(),
            CssSyntaxKind::CSS_GENERIC_PROPERTY | CssSyntaxKind::CSS_COMPOSES_PROPERTY
        )
    })?;

    let mut property_children = property_node.children();
    let name_node = property_children.next()?;
    let property = ctx.text(name_node.text_trimmed_range()).trim().to_string();

    // The value is everything after the `:` that is not the `!important` flag,
    // so replacing this range alone preserves both the flag and any inline
    // comment sitting on the declaration (rule E).
    let value_node = property_node.children().nth(1);
    let (value_start, value_end) = match value_node {
        Some(v) => range_to_pair(v.text_trimmed_range()),
        None => {
            let colon = property_node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| t.kind() == CssSyntaxKind::COLON);
            let after = colon
                .map(|t| usize::from(t.text_range().end()))
                .unwrap_or_else(|| usize::from(property_node.text_trimmed_range().end()));
            let end = usize::from(property_node.text_trimmed_range().end());
            (after.min(end), end)
        }
    };

    let important_range = decl
        .children()
        .find(|c| c.kind() == CssSyntaxKind::CSS_DECLARATION_IMPORTANT)
        .map(|c| range_to_pair(c.text_trimmed_range()));
    let important = important_range.is_some();

    let (start, end) = trimmed(&item);
    Some(DeclRef {
        node: item,
        property,
        value_raw: ctx
            .source()
            .get(value_start..value_end)
            .unwrap_or("")
            .to_string(),
        important,
        start,
        end,
        value_start,
        value_end,
        important_range,
    })
}

/// Children of the root `CSS_RULE_LIST`, in source order.
pub fn top_level_nodes(ctx: &ParseCtx) -> Vec<CssSyntaxNode> {
    ctx.syntax()
        .children()
        .filter(|c| c.kind() == CssSyntaxKind::CSS_RULE_LIST)
        .flat_map(|l| l.children())
        .collect()
}

pub fn find_top_level_rules(ctx: &ParseCtx) -> Vec<RuleRef> {
    top_level_nodes(ctx)
        .iter()
        .filter_map(|n| rule_ref_from(n, ctx))
        .collect()
}

/// Every qualified rule anywhere in the file, including inside `@media`,
/// `@supports`, `@layer` and nested rules.
pub fn find_all_rules(ctx: &ParseCtx) -> Vec<RuleRef> {
    ctx.syntax()
        .descendants()
        .filter_map(|n| rule_ref_from(&n, ctx))
        .collect()
}

pub fn find_top_level_at_rules(ctx: &ParseCtx) -> Vec<AtRuleRef> {
    top_level_nodes(ctx)
        .iter()
        .filter_map(|n| at_rule_ref_from(n, ctx))
        .collect()
}

pub fn find_all_at_rules(ctx: &ParseCtx) -> Vec<AtRuleRef> {
    ctx.syntax()
        .descendants()
        .filter_map(|n| at_rule_ref_from(&n, ctx))
        .collect()
}

/// Top-level at-rules with the given (lowercase, no `@`) name.
pub fn find_at_rules_named(ctx: &ParseCtx, name: &str) -> Vec<AtRuleRef> {
    let want = name.trim_start_matches('@').to_lowercase();
    find_top_level_at_rules(ctx)
        .into_iter()
        .filter(|r| r.name == want)
        .collect()
}

/// Find a top-level rule by selector. `Ambiguous` when more than one matches.
pub fn find_rule_by_selector(ctx: &ParseCtx, selector: &str) -> MatchResult {
    find_rule_among(find_top_level_rules(ctx), selector)
}

/// Same, but searching every rule in the file including nested ones. Callers
/// must opt in explicitly (ROADMAP Phase 2).
pub fn find_rule_by_selector_anywhere(ctx: &ParseCtx, selector: &str) -> MatchResult {
    find_rule_among(find_all_rules(ctx), selector)
}

fn find_rule_among(rules: Vec<RuleRef>, selector: &str) -> MatchResult {
    let want = normalize_selector(selector);
    if want.is_empty() {
        return MatchResult::None;
    }
    let mut hits: Vec<RuleRef> = rules
        .into_iter()
        .filter(|r| r.selector_norm == want)
        .collect();
    match hits.len() {
        0 => MatchResult::None,
        1 => MatchResult::One(Box::new(hits.remove(0))),
        _ => MatchResult::Ambiguous(hits),
    }
}

/// Declarations directly inside a rule body, in source order. Nested rules and
/// nested at-rules (`@apply`, `@media`) are not declarations and are skipped.
pub fn declarations_in(ctx: &ParseCtx, rule: &RuleRef) -> Vec<DeclRef> {
    let Some(block) = block_child(&rule.node) else {
        return Vec::new();
    };
    block_items(&block)
        .iter()
        .filter(|n| is_declaration_item(n.kind()))
        .filter_map(|n| decl_ref_from(n, ctx))
        .collect()
}

/// Declarations directly inside any block node (used for `@theme`, `@plugin`
/// and other at-rules that carry a declaration body).
pub fn declarations_in_block(ctx: &ParseCtx, block_node: &CssSyntaxNode) -> Vec<DeclRef> {
    let Some(block) = block_child(block_node) else {
        return Vec::new();
    };
    block_items(&block)
        .iter()
        .filter(|n| is_declaration_item(n.kind()))
        .filter_map(|n| decl_ref_from(n, ctx))
        .collect()
}

/// The last declaration of `property` in `rule`, or `None`.
///
/// Last, not first: when a rule repeats a property the last one wins in CSS, so
/// that is the one a caller means when they say "set this property".
pub fn find_declaration(ctx: &ParseCtx, rule: &RuleRef, property: &str) -> Option<DeclRef> {
    let want = normalize_property(property);
    declarations_in(ctx, rule)
        .into_iter()
        .rfind(|d| normalize_property(&d.property) == want)
}

pub fn find_declarations(ctx: &ParseCtx, rule: &RuleRef, property: &str) -> Vec<DeclRef> {
    let want = normalize_property(property);
    declarations_in(ctx, rule)
        .into_iter()
        .filter(|d| normalize_property(&d.property) == want)
        .collect()
}

/// Declaration-holding list nodes in the file, each paired with the
/// declarations directly inside it. Used by ops that work block-by-block.
pub fn declaration_lists(ctx: &ParseCtx) -> Vec<(CssSyntaxNode, Vec<DeclRef>)> {
    ctx.syntax()
        .descendants()
        .filter(|n| {
            matches!(
                n.kind(),
                CssSyntaxKind::CSS_DECLARATION_OR_RULE_LIST
                    | CssSyntaxKind::CSS_DECLARATION_LIST
                    | CssSyntaxKind::CSS_DECLARATION_OR_AT_RULE_LIST
            )
        })
        .map(|list| {
            let decls = list
                .children()
                .filter(|c| is_declaration_item(c.kind()))
                .filter_map(|c| decl_ref_from(&c, ctx))
                .collect();
            (list, decls)
        })
        .collect()
}

/// Every declaration in the file, wherever it lives. Used by the read-only
/// analysis ops.
pub fn all_declarations(ctx: &ParseCtx) -> Vec<DeclRef> {
    ctx.syntax()
        .descendants()
        .filter(|n| is_declaration_item(n.kind()))
        // A bare CSS_DECLARATION nested inside CSS_DECLARATION_WITH_SEMICOLON
        // would otherwise be counted twice.
        .filter(|n| {
            n.kind() != CssSyntaxKind::CSS_DECLARATION
                || n.parent().map(|p| p.kind())
                    != Some(CssSyntaxKind::CSS_DECLARATION_WITH_SEMICOLON)
        })
        .filter_map(|n| decl_ref_from(&n, ctx))
        .collect()
}

/// Insertion anchor: end of the last top-level at-rule whose name is in
/// `names`. `None` when the file has none of them.
pub fn last_top_level_at_rule_end(ctx: &ParseCtx, names: &[&str]) -> Option<usize> {
    let wanted: Vec<String> = names
        .iter()
        .map(|n| n.trim_start_matches('@').to_lowercase())
        .collect();
    find_top_level_at_rules(ctx)
        .iter()
        .filter(|r| wanted.contains(&r.name))
        .map(|r| r.end)
        .next_back()
}

/// Where a new top-level line should go when the file has no anchor at-rule of
/// its own family: after any leading comment block and `@charset`, but before
/// the first real rule (ROADMAP §8, `ensure_at_rule_line`).
pub fn top_of_file_anchor(ctx: &ParseCtx) -> usize {
    let src = ctx.source();
    // `ctx.source()` never contains a BOM -- it is stripped at parse time and
    // re-attached on output -- so offset 0 is always a safe starting anchor.
    let mut anchor = 0usize;

    // `@charset` must stay first in the file.
    if let Some(cs) = find_at_rules_named(ctx, "charset").first() {
        anchor = ctx.line_end_inclusive(cs.end);
    }

    // Skip a leading comment block: consecutive comment lines and blank lines
    // that appear before the first rule.
    let first_rule_start = top_level_nodes(ctx)
        .iter()
        .filter(|n| !is_bogus(n.kind()))
        .map(|n| usize::from(n.text_trimmed_range().start()))
        .find(|start| *start >= anchor);

    let Some(first_rule_start) = first_rule_start else {
        // No rules at all: land at the end of whatever leading text exists.
        return src.len();
    };

    // Leading trivia of the first rule holds the comments above it. Walk its
    // comment pieces and stop at the first blank line, which we read as the
    // boundary between a file header and a comment about the rule itself.
    let Some(token) = ctx
        .syntax()
        .token_at_offset(biome_rowan::TextSize::from(first_rule_start as u32))
        .right_biased()
    else {
        return anchor;
    };

    let mut cursor = anchor;
    let mut newlines_since_comment = 0usize;
    for piece in token.leading_trivia().pieces() {
        let piece_start = usize::from(piece.text_range().start());
        if piece_start < anchor {
            continue;
        }
        if crate::ctx::is_comment_kind(piece.kind()) {
            cursor = usize::from(piece.text_range().end());
            newlines_since_comment = 0;
        } else if piece.kind() == biome_rowan::TriviaPieceKind::Newline {
            newlines_since_comment += 1;
            if newlines_since_comment >= 2 && cursor > anchor {
                // Blank line after the header block: stop here.
                break;
            }
        }
    }

    if cursor > anchor {
        ctx.line_end_inclusive(cursor)
    } else {
        anchor
    }
}

/// Comments in the file, as `(start, end, text)` triples.
pub fn all_comments(ctx: &ParseCtx) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    for token in ctx.syntax().descendants_tokens(Direction::Next) {
        for piece in token
            .leading_trivia()
            .pieces()
            .chain(token.trailing_trivia().pieces())
        {
            if crate::ctx::is_comment_kind(piece.kind()) {
                let (s, e) = range_to_pair(piece.text_range());
                out.push((s, e, piece.text().to_string()));
            }
        }
    }
    out.sort_by_key(|(s, _, _)| *s);
    out.dedup_by_key(|(s, _, _)| *s);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::ParseCtx;

    fn ctx(src: &str) -> ParseCtx {
        ParseCtx::parse_default(src)
    }

    // -- normalisation ------------------------------------------------------

    #[test]
    fn normalises_whitespace_runs() {
        assert_eq!(normalize_selector("  .a    .b  "), ".a .b");
        assert_eq!(normalize_selector(".a\n\t.b"), ".a .b");
    }

    #[test]
    fn normalises_combinators() {
        assert_eq!(normalize_selector(".a>.b"), ".a > .b");
        assert_eq!(normalize_selector(".a  >  .b"), ".a > .b");
        assert_eq!(normalize_selector(".a+.b"), ".a + .b");
        assert_eq!(normalize_selector(".a~.b"), ".a ~ .b");
    }

    #[test]
    fn normalises_selector_lists() {
        assert_eq!(normalize_selector(".a,.b ,  .c"), ".a, .b, .c");
    }

    #[test]
    fn leaves_quoted_text_alone() {
        assert_eq!(
            normalize_selector(r#"a[href^="https://a  b"]"#),
            r#"a[href^="https://a  b"]"#
        );
    }

    #[test]
    fn does_not_treat_tilde_inside_brackets_as_a_combinator() {
        assert_eq!(normalize_selector(r#"[data-x~="y"]"#), r#"[data-x~="y"]"#);
    }

    #[test]
    fn does_not_treat_plus_inside_parens_as_a_combinator() {
        assert_eq!(
            normalize_selector("li:nth-child(2n + 1)"),
            "li:nth-child(2n + 1)"
        );
        assert_eq!(
            normalize_selector("li:nth-child(2n+1)"),
            "li:nth-child(2n+1)"
        );
    }

    #[test]
    fn different_selectors_do_not_collide() {
        assert_ne!(normalize_selector(".a .b"), normalize_selector(".a > .b"));
        assert_ne!(normalize_selector(".ab"), normalize_selector(".a .b"));
    }

    #[test]
    fn normalisation_is_idempotent() {
        for s in [
            ".a>.b",
            ".a , .b",
            "  .a   .b  ",
            r#"a[href^="x"]:not(.y)::after"#,
            "li:nth-child(2n+1)",
        ] {
            let once = normalize_selector(s);
            assert_eq!(normalize_selector(&once), once, "not idempotent: {s}");
        }
    }

    #[test]
    fn property_normalisation_lowercases_but_keeps_custom_property_case() {
        assert_eq!(normalize_property("  COLOR "), "color");
        assert_eq!(normalize_property("--Brand"), "--Brand");
    }

    // -- rules --------------------------------------------------------------

    #[test]
    fn finds_top_level_rules_only() {
        let c = ctx(".a { color: red; }\n@media print { .b { color: blue; } }\n");
        let rules = find_top_level_rules(&c);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selector_norm, ".a");
    }

    #[test]
    fn find_all_rules_descends_into_at_rules() {
        let c = ctx(".a { color: red; }\n@media print { .b { color: blue; } }\n");
        let all = find_all_rules(&c);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn rule_ranges_exclude_leading_comments() {
        let src = "/* above */\n.a { color: red; }\n";
        let c = ctx(src);
        let r = &find_top_level_rules(&c)[0];
        assert_eq!(&src[r.start..r.end], ".a { color: red; }");
    }

    #[test]
    fn rule_body_bounds_are_correct() {
        let src = ".a {\n  color: red;\n}\n";
        let c = ctx(src);
        let r = &find_top_level_rules(&c)[0];
        assert_eq!(&src[r.body_open..r.body_close], "\n  color: red;\n");
    }

    #[test]
    fn empty_body_is_detected() {
        let c = ctx(".a {}\n.b { color: red; }\n");
        let rules = find_top_level_rules(&c);
        assert!(rules[0].body_is_empty(&c));
        assert!(!rules[1].body_is_empty(&c));
    }

    #[test]
    fn matches_a_selector_written_differently() {
        let c = ctx(".a   >   .b { color: red; }\n");
        assert!(matches!(
            find_rule_by_selector(&c, ".a>.b"),
            MatchResult::One(_)
        ));
    }

    #[test]
    fn reports_ambiguity_rather_than_guessing() {
        let c = ctx(".a { color: red; }\n.a { color: blue; }\n");
        let m = find_rule_by_selector(&c, ".a");
        assert!(matches!(m, MatchResult::Ambiguous(_)));
        assert_eq!(m.count(), 2);
    }

    #[test]
    fn does_not_match_a_substring_of_a_selector() {
        let c = ctx(".header-inner { color: red; }\n");
        assert!(find_rule_by_selector(&c, ".header").is_none());
    }

    #[test]
    fn does_not_match_one_member_of_a_selector_list() {
        let c = ctx(".a, .b { color: red; }\n");
        assert!(find_rule_by_selector(&c, ".a").is_none());
        assert!(matches!(
            find_rule_by_selector(&c, ".a, .b"),
            MatchResult::One(_)
        ));
    }

    #[test]
    fn does_not_reach_into_media_blocks_by_default() {
        let c = ctx("@media print { .b { color: blue; } }\n");
        assert!(find_rule_by_selector(&c, ".b").is_none());
        assert!(matches!(
            find_rule_by_selector_anywhere(&c, ".b"),
            MatchResult::One(_)
        ));
    }

    // -- declarations -------------------------------------------------------

    #[test]
    fn reads_declarations_in_order() {
        let c = ctx(".a {\n  color: red;\n  margin: 0;\n}\n");
        let r = find_rule_by_selector(&c, ".a").one().unwrap();
        let decls = declarations_in(&c, &r);
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].property, "color");
        assert_eq!(decls[1].property, "margin");
    }

    #[test]
    fn declaration_value_range_covers_only_the_value() {
        let src = ".a { color: red; }";
        let c = ctx(src);
        let r = find_rule_by_selector(&c, ".a").one().unwrap();
        let d = find_declaration(&c, &r, "color").unwrap();
        assert_eq!(&src[d.value_start..d.value_end], "red");
    }

    #[test]
    fn declaration_value_range_excludes_important() {
        let src = ".a { color: red !important; }";
        let c = ctx(src);
        let r = find_rule_by_selector(&c, ".a").one().unwrap();
        let d = find_declaration(&c, &r, "color").unwrap();
        assert_eq!(&src[d.value_start..d.value_end], "red");
        assert!(d.important);
    }

    #[test]
    fn declaration_value_range_excludes_a_trailing_comment() {
        let src = ".a { color: red; /* why */ }";
        let c = ctx(src);
        let r = find_rule_by_selector(&c, ".a").one().unwrap();
        let d = find_declaration(&c, &r, "color").unwrap();
        assert_eq!(&src[d.value_start..d.value_end], "red");
    }

    #[test]
    fn multi_token_values_are_captured_whole() {
        let src = ".a { margin: 0 auto 10px; }";
        let c = ctx(src);
        let r = find_rule_by_selector(&c, ".a").one().unwrap();
        let d = find_declaration(&c, &r, "margin").unwrap();
        assert_eq!(&src[d.value_start..d.value_end], "0 auto 10px");
    }

    #[test]
    fn custom_properties_are_found() {
        let src = ":root { --brand: #4f46e5; }";
        let c = ctx(src);
        let r = find_rule_by_selector(&c, ":root").one().unwrap();
        let d = find_declaration(&c, &r, "--brand").unwrap();
        assert_eq!(&src[d.value_start..d.value_end], "#4f46e5");
    }

    #[test]
    fn a_repeated_property_resolves_to_the_last_one() {
        let src = ".a { color: red; color: blue; }";
        let c = ctx(src);
        let r = find_rule_by_selector(&c, ".a").one().unwrap();
        let d = find_declaration(&c, &r, "color").unwrap();
        assert_eq!(&src[d.value_start..d.value_end], "blue");
        assert_eq!(find_declarations(&c, &r, "color").len(), 2);
    }

    #[test]
    fn declaration_lookup_is_case_insensitive_for_standard_properties() {
        let c = ctx(".a { COLOR: red; }");
        let r = find_rule_by_selector(&c, ".a").one().unwrap();
        assert!(find_declaration(&c, &r, "color").is_some());
    }

    #[test]
    fn nested_at_rules_are_not_declarations() {
        let c = ctx(".a { @apply px-2; color: red; }");
        let r = find_rule_by_selector(&c, ".a").one().unwrap();
        let decls = declarations_in(&c, &r);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].property, "color");
    }

    // -- at-rules -----------------------------------------------------------

    #[test]
    fn reads_at_rule_names_and_preludes() {
        let c = ctx("@import \"tailwindcss\";\n@plugin \"../vendor/daisyui\";\n");
        let rules = find_top_level_at_rules(&c);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].name, "import");
        assert_eq!(rules[0].prelude, "\"tailwindcss\"");
        assert_eq!(rules[1].name, "plugin");
        assert_eq!(rules[1].prelude, "\"../vendor/daisyui\"");
    }

    #[test]
    fn reads_block_at_rules() {
        let src = "@theme {\n  --a: 1;\n}\n";
        let c = ctx(src);
        let r = &find_at_rules_named(&c, "theme")[0];
        assert!(r.has_block);
        assert_eq!(r.prelude, "");
        assert_eq!(
            &src[r.body_open.unwrap()..r.body_close.unwrap()],
            "\n  --a: 1;\n"
        );
    }

    #[test]
    fn reads_block_at_rules_with_a_prelude() {
        let c = ctx("@plugin \"../vendor/daisyui\" {\n  themes: false;\n}\n");
        let r = &find_at_rules_named(&c, "plugin")[0];
        assert!(r.has_block);
        assert_eq!(r.prelude, "\"../vendor/daisyui\"");
    }

    #[test]
    fn at_rule_name_lookup_tolerates_the_at_sign_and_case() {
        let c = ctx("@IMPORT \"x\";\n");
        assert_eq!(find_at_rules_named(&c, "@import").len(), 1);
    }

    #[test]
    fn finds_declarations_inside_a_theme_block() {
        let c = ctx("@theme {\n  --color-brand: red;\n  --font-x: sans;\n}\n");
        let at = &find_at_rules_named(&c, "theme")[0];
        let decls = declarations_in_block(&c, &at.node.children().next().unwrap());
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].property, "--color-brand");
    }

    #[test]
    fn anchors_after_the_last_at_rule_of_a_family() {
        let src = "@import \"a\";\n@import \"b\";\n.x { color: red; }\n";
        let c = ctx(src);
        let end = last_top_level_at_rule_end(&c, &["import"]).unwrap();
        assert_eq!(&src[..end], "@import \"a\";\n@import \"b\";");
    }

    #[test]
    fn no_anchor_when_the_family_is_absent() {
        let c = ctx(".x { color: red; }\n");
        assert!(last_top_level_at_rule_end(&c, &["import"]).is_none());
    }

    // -- anchors ------------------------------------------------------------

    #[test]
    fn top_of_file_anchor_is_zero_for_a_bare_rule() {
        let c = ctx(".a { color: red; }\n");
        assert_eq!(top_of_file_anchor(&c), 0);
    }

    #[test]
    fn top_of_file_anchor_skips_a_header_comment_block() {
        let src = "/* Header line one.\n   Header line two. */\n\n.a { color: red; }\n";
        let c = ctx(src);
        let a = top_of_file_anchor(&c);
        assert_eq!(&src[..a], "/* Header line one.\n   Header line two. */\n");
    }

    #[test]
    fn top_of_file_anchor_keeps_a_comment_attached_to_the_first_rule() {
        // No blank line: the comment belongs to `.a`, so we insert above it.
        let src = "/* about .a */\n.a { color: red; }\n";
        let c = ctx(src);
        assert_eq!(top_of_file_anchor(&c), 14 + 1);
    }

    #[test]
    fn top_of_file_anchor_lands_after_charset() {
        let src = "@charset \"utf-8\";\n.a { color: red; }\n";
        let c = ctx(src);
        let a = top_of_file_anchor(&c);
        assert_eq!(&src[..a], "@charset \"utf-8\";\n");
    }

    #[test]
    fn top_of_file_anchor_is_bom_relative() {
        // The BOM is not part of `ctx.source()`, so the anchor is 0 and the
        // caller re-attaches the BOM afterwards.
        let c = ctx("\u{feff}.a { color: red; }\n");
        assert_eq!(top_of_file_anchor(&c), 0);
        assert!(c.has_bom());
    }

    // -- comments -----------------------------------------------------------

    #[test]
    fn collects_every_comment_exactly_once() {
        let src = "/* a */\n.x { /* b */ color: red; /* c */ }\n/* d */\n";
        let c = ctx(src);
        let comments = all_comments(&c);
        let texts: Vec<&str> = comments.iter().map(|(_, _, t)| t.as_str()).collect();
        assert_eq!(texts, vec!["/* a */", "/* b */", "/* c */", "/* d */"]);
    }
}
