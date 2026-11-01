// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Typed queries over the CST that return **byte ranges**, never owned
//! strings to be reprinted.
//!
//! Matching rules are deliberately strict:
//!   * top-level rules only, unless the caller explicitly opts into descending;
//!   * selectors compared on a normalised form, never raw equality and never
//!     substring/fuzzy;
//!   * more than one match is `MatchResult::Ambiguous` -- we error, we do not
//!     pick one, because guessing produces surprising diffs.

use crate::ctx::{is_bogus, ParseCtx};
use biome_css_syntax::{CssSyntaxKind, CssSyntaxNode, CssSyntaxToken};
use biome_rowan::{Direction, TextRange, TextSize};

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
    /// The prelude rendered token by token with single-space separation, for
    /// comparisons. Built from the CST rather than by collapsing whitespace in
    /// the raw text, so a comment or an odd line break inside the prelude
    /// cannot change the answer.
    pub prelude_norm: String,
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
    /// False for an `@` the parser could not place in an at-rule -- see
    /// [`find_every_at_rule`]. Its `node` is then the one the `@` sits in.
    pub placed: bool,
}

impl AtRuleRef {
    /// Is this the at-rule a caller's `matching` names? Its target when it has
    /// one -- `@plugin "daisyui"` is `daisyui` -- and its whole prelude when it
    /// has none, so `""` is an at-rule with nothing after its name: `@theme`,
    /// not `@theme inline`. `None` names every one. `want` is already through
    /// `normalize_target_needle`.
    pub fn matches(&self, want: Option<&str>) -> bool {
        match (want, &self.target) {
            (None, _) => true,
            (Some(want), Some(target)) => target == want,
            (Some(want), None) => self.prelude_norm == want,
        }
    }
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
    /// when the property already exists.
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
/// block kinds and new ones appear between Biome releases.
/// The at-rule's inner node -- the one that carries the name, the prelude and
/// the block. This is what [`declarations_in_block`] wants: it takes the block's
/// *owner*, not the block, so it can be handed a rule or an at-rule alike.
///
/// `None` when the at-rule has no block.
pub fn at_rule_body(at: &AtRuleRef) -> Option<CssSyntaxNode> {
    let inner = at.node.children().next()?;
    block_child(&inner).map(|_| inner)
}

fn block_child(node: &CssSyntaxNode) -> Option<CssSyntaxNode> {
    node.children().find(is_block)
}

/// Does `node` open with `{`?
fn is_block(node: &CssSyntaxNode) -> bool {
    node.first_token()
        .is_some_and(|t| t.kind() == CssSyntaxKind::L_CURLY)
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

/// Render a selector subtree in canonical form.
///
/// Structural, not textual: the CST already models combinators as tokens
/// (`>` `+` `~`, and a `CSS_SPACE_LITERAL` for descendant) and selector lists
/// as element children, so canonical spacing falls out of walking the tree.
/// Everything below a combinator is concatenated token by token, which is
/// correct because whitespace is not legal inside a compound selector -- and it
/// means quoted attribute values and `:not(...)` arguments are copied verbatim
/// without any quote or bracket tracking.
fn render_selector(node: &CssSyntaxNode, out: &mut String) {
    match node.kind() {
        // `.a, .b` -- join the elements, drop the source's own commas.
        CssSyntaxKind::CSS_SELECTOR_LIST
        | CssSyntaxKind::CSS_COMPOUND_SELECTOR_LIST
        | CssSyntaxKind::CSS_ANY_SELECTOR_LIST
        | CssSyntaxKind::CSS_RELATIVE_SELECTOR_LIST => {
            for (i, child) in node.children().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                render_selector(&child, out);
            }
        }
        // `left <combinator> right`, always exactly one space either side.
        CssSyntaxKind::CSS_COMPLEX_SELECTOR => {
            for element in node.children_with_tokens() {
                match element {
                    biome_rowan::SyntaxElement::Node(child) => render_selector(&child, out),
                    biome_rowan::SyntaxElement::Token(token) => {
                        // The descendant combinator is a space literal; every
                        // other combinator carries its own glyph.
                        if token.kind() == CssSyntaxKind::CSS_SPACE_LITERAL {
                            out.push(' ');
                        } else {
                            out.push(' ');
                            out.push_str(token.text_trimmed());
                            out.push(' ');
                        }
                    }
                }
            }
        }
        _ => {
            for element in node.children_with_tokens() {
                match element {
                    biome_rowan::SyntaxElement::Node(child) => render_selector(&child, out),
                    biome_rowan::SyntaxElement::Token(token) => out.push_str(token.text_trimmed()),
                }
            }
        }
    }
}

/// Canonical form of a selector node, for comparison purposes.
pub fn normalize_selector_node(node: &CssSyntaxNode) -> String {
    let mut out = String::new();
    render_selector(node, &mut out);
    // No post-processing: collapsing whitespace here would reach inside string
    // literals such as `a[href^="a  b"]`. The walk emits exactly one space per
    // combinator, so there is nothing to collapse.
    out.trim().to_string()
}

/// Canonical form of a caller-supplied selector string.
///
/// Parsed with Biome and rendered through [`normalize_selector_node`], so a
/// selector written by hand in Elixir and one read out of a stylesheet go
/// through exactly the same code path and cannot disagree.
pub fn normalize_selector(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if crate::ctx::check_nesting(trimmed).is_err() {
        return trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    let probe = format!("{trimmed} {{}}");
    let Ok(ctx) = ParseCtx::try_new(&probe, crate::ctx::ParseOptions::default()) else {
        return trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    };
    let selector_list = ctx
        .syntax()
        .descendants()
        .find(|n| n.kind() == CssSyntaxKind::CSS_SELECTOR_LIST);

    match selector_list {
        Some(list) if !list.text_trimmed().to_string().is_empty() => normalize_selector_node(&list),
        // Unparseable as a selector: fall back to whitespace collapsing so the
        // caller still gets a deterministic key rather than an empty one.
        _ => trimmed.split_whitespace().collect::<Vec<_>>().join(" "),
    }
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
    // Native CSS nesting gives `&:hover { }` a different shape from a top-level
    // rule: `CSS_NESTED_QUALIFIED_RULE` holding a `CSS_RELATIVE_SELECTOR_LIST`.
    // Both are rules, and queries that descend must see both.
    if !matches!(
        node.kind(),
        CssSyntaxKind::CSS_QUALIFIED_RULE | CssSyntaxKind::CSS_NESTED_QUALIFIED_RULE
    ) {
        return None;
    }
    let selector_list = node.children().find(|c| {
        matches!(
            c.kind(),
            CssSyntaxKind::CSS_SELECTOR_LIST | CssSyntaxKind::CSS_RELATIVE_SELECTOR_LIST
        )
    })?;
    let block = block_child(node)?;
    let (body_open, body_close) = block_bounds(&block)?;
    let (start, end) = trimmed(node);
    let selector_raw = ctx.text(selector_list.text_trimmed_range()).to_string();

    Some(RuleRef {
        selector_norm: normalize_selector_node(&selector_list),
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

pub fn at_rule_ref(ctx: &ParseCtx, node: &CssSyntaxNode) -> Option<AtRuleRef> {
    at_rule_ref_from(node, ctx)
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

    // Canonical prelude: the tokens between the name and the `;`/`{`, joined by
    // single spaces. Trivia (whitespace and comments) is excluded by
    // construction because we read `text_trimmed` of each token.
    // `take_while`, not `filter`: tokens come in document order, so stopping at
    // the prelude's end keeps this proportional to the prelude rather than to
    // the whole subtree -- which for an outer at-rule is the entire file.
    let prelude_norm = inner
        .descendants_tokens(Direction::Next)
        .take_while(|t| usize::from(t.text_trimmed_range().start()) < prelude_end)
        .filter(|t| usize::from(t.text_trimmed_range().start()) >= name_end)
        .map(|t| t.text_trimmed().to_string())
        .collect::<Vec<_>>()
        .join(" ");

    Some(AtRuleRef {
        prelude_norm,
        target: at_rule_target_token(node),
        node: node.clone(),
        name,
        prelude,
        start,
        end,
        has_block,
        body_open,
        body_close,
        placed: true,
    })
}

/// The at-rule an `@` starts where the parser could not place one, read from
/// the tokens that follow it: the name written against the `@`, then up to the
/// `;` that ends it or the `}` that closes its block. A `}` closing the block
/// the `@` sits in ends it first.
///
/// `None` for an `@` with no name against it, or one that does not start a
/// statement -- first in the file or after a `;`, `{` or `}`, comments aside --
/// because after anything else it is part of a selector or a value
/// (`url(@x.png)`), as every reader of CSS takes it.
fn stray_at_rule_ref(ctx: &ParseCtx, at: &CssSyntaxToken) -> Option<AtRuleRef> {
    let starts_statement = at.prev_token().is_none_or(|token| {
        matches!(
            token.kind(),
            CssSyntaxKind::SEMICOLON | CssSyntaxKind::L_CURLY | CssSyntaxKind::R_CURLY
        )
    });
    if !starts_statement {
        return None;
    }
    let (start, at_end) = range_to_pair(at.text_trimmed_range());
    let name_token = at.next_token()?;
    let (name_start, name_end) = range_to_pair(name_token.text_trimmed_range());
    if name_start != at_end || name_token.kind() == CssSyntaxKind::EOF {
        return None;
    }

    let mut end = name_end;
    let mut depth = 0usize;
    let mut prelude_end = None;
    let mut body_open = None;
    let mut body_close = None;
    let mut target = None;
    let mut prelude_norm = Vec::new();
    let mut cursor = name_token.next_token();

    while let Some(token) = cursor {
        let (token_start, token_end) = range_to_pair(token.text_trimmed_range());
        match token.kind() {
            CssSyntaxKind::EOF => break,
            CssSyntaxKind::SEMICOLON if depth == 0 => {
                prelude_end.get_or_insert(token_start);
                end = token_end;
                break;
            }
            CssSyntaxKind::L_CURLY => {
                if depth == 0 {
                    prelude_end.get_or_insert(token_start);
                    body_open = Some(token_end);
                }
                depth += 1;
                end = token_end;
            }
            CssSyntaxKind::R_CURLY if depth == 0 => break,
            CssSyntaxKind::R_CURLY => {
                depth -= 1;
                end = token_end;
                if depth == 0 {
                    body_close = Some(token_start);
                    break;
                }
            }
            kind => {
                if prelude_end.is_none() {
                    if target.is_none() {
                        target = match kind {
                            CssSyntaxKind::CSS_STRING_LITERAL => {
                                Some(unquote(token.text_trimmed()))
                            }
                            CssSyntaxKind::CSS_URL_VALUE_RAW_LITERAL => {
                                Some(token.text_trimmed().trim().to_string())
                            }
                            _ => None,
                        };
                    }
                    prelude_norm.push(token.text_trimmed().to_string());
                }
                end = token_end;
            }
        }
        cursor = token.next_token();
    }

    let prelude_end = prelude_end.unwrap_or(end);
    Some(AtRuleRef {
        node: at.parent()?,
        name: name_token.text_trimmed().trim().to_lowercase(),
        prelude: ctx
            .source()
            .get(name_end..prelude_end.max(name_end))
            .unwrap_or("")
            .trim()
            .to_string(),
        prelude_norm: prelude_norm.join(" "),
        target,
        start,
        end,
        has_block: body_open.is_some(),
        body_open,
        // An unclosed block runs to the end of the at-rule's text.
        body_close: body_open.map(|_| body_close.unwrap_or(end)),
        placed: false,
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
    // comment sitting on the declaration.
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

/// Every at-rule in the file, in source order: each one [`find_all_at_rules`]
/// finds, at any depth, and each `@` the parser could not place in an at-rule,
/// such as an `@import` inside `@font-face` or a keyframe. Biome keeps those as
/// bogus tokens, but a reader with a looser grammar -- Tailwind, for one -- still
/// acts on them, so a caller deciding what a stylesheet may ask for has to see
/// them too. An `@` inside a string, a comment or a `url()` is part of that
/// token and starts nothing.
pub fn find_every_at_rule(ctx: &ParseCtx) -> Vec<AtRuleRef> {
    ctx.syntax()
        .descendants_tokens(Direction::Next)
        .filter(|token| token.kind() == CssSyntaxKind::AT)
        .filter_map(|at| match at.parent() {
            Some(node) if node.kind() == CssSyntaxKind::CSS_AT_RULE => at_rule_ref_from(&node, ctx),
            _ => stray_at_rule_ref(ctx, &at),
        })
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
/// must opt in explicitly.
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

/// An at-rule's block seen as a rule, so the rule-body machinery --
/// [`declarations_in`], `append_to_body` -- works on `@theme { ... }` exactly as
/// it does on `.btn { ... }`. `None` for an at-rule with no block.
pub fn at_rule_block_rule(at: &AtRuleRef) -> Option<RuleRef> {
    Some(RuleRef {
        node: at_rule_body(at)?,
        selector_raw: format!("@{}", at.name),
        selector_norm: format!("@{}", at.name),
        start: at.start,
        end: at.end,
        body_open: at.body_open?,
        body_close: at.body_close?,
    })
}

/// Items directly inside `rule`'s body that are not declarations: a nested
/// rule, a nested at-rule, or text the parser could not read as a declaration.
/// A stray `;` is nothing, and is not reported.
pub fn non_declaration_items(ctx: &ParseCtx, rule: &RuleRef) -> Vec<CssSyntaxNode> {
    let Some(block) = block_child(&rule.node) else {
        return Vec::new();
    };
    block_items(&block)
        .into_iter()
        .filter(|n| !is_declaration_item(n.kind()))
        .filter(|n| !matches!(ctx.text(n.text_trimmed_range()).trim(), "" | ";"))
        .collect()
}

/// The value of `decl` as its tokens joined by single spaces, for comparing two
/// values: whitespace and comments between tokens are trivia, so a value broken
/// over three lines equals the same value on one. Read from the CST, like
/// `AtRuleRef::prelude_norm`.
pub fn value_norm(decl: &DeclRef) -> String {
    decl.node
        .descendants_tokens(Direction::Next)
        .filter(|t| {
            let start = usize::from(t.text_trimmed_range().start());
            start >= decl.value_start && start < decl.value_end
        })
        .map(|t| t.text_trimmed().to_string())
        .collect::<Vec<_>>()
        .join(" ")
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
/// the first real rule.
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

// ---------------------------------------------------------------------------
// Errors the parser raises against valid CSS, and errors it does not raise
// ---------------------------------------------------------------------------

/// Byte offset of the `}` that closes `rule`'s own block, or `None` when the
/// block is never closed.
pub fn closing_brace(rule: &RuleRef) -> Option<usize> {
    let block = block_child(&rule.node)?;
    block
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == CssSyntaxKind::R_CURLY)
        .map(|t| usize::from(t.text_trimmed_range().start()))
}

/// The smallest node whose range covers `start..end`.
fn covering_node(ctx: &ParseCtx, start: usize, end: usize) -> Option<CssSyntaxNode> {
    let start = u32::try_from(start).ok()?;
    let end = u32::try_from(end).ok()?;
    let range = TextRange::new(start.into(), end.into());
    if !ctx.syntax().text_range().contains_range(range) {
        return None;
    }
    match ctx.syntax().covering_element(range) {
        biome_rowan::SyntaxElement::Node(node) => Some(node),
        biome_rowan::SyntaxElement::Token(token) => token.parent(),
    }
}

/// Is `start..end` inside a declaration value whose grammar is any run of
/// tokens, where Biome 0.5.8 raises an error the specification does not?
///
/// * A custom property's value, `--x: a:b`: CSS Custom Properties takes any
///   tokens there, a `:` included. Biome reads it as an ordinary value and
///   refuses the colon.
/// * A legacy `progid:` filter, `filter: progid:DXImageTransform…(…)`, which old
///   Internet Explorer read and vendored stylesheets such as Bootstrap 3 still
///   carry.
///
/// Only while every `(`, `[` and `{` in the value is closed inside it. The
/// specification reads an open one to its match, braces included, so it
/// swallows everything after it; Biome ends the declaration at the next `;`
/// instead, and the value would look harmless here when it is not.
pub fn in_free_form_value(ctx: &ParseCtx, start: usize, end: usize) -> bool {
    let Some(node) = covering_node(ctx, start, end) else {
        return false;
    };
    let Some(property) = node.ancestors().find(|n| {
        matches!(
            n.kind(),
            CssSyntaxKind::CSS_GENERIC_PROPERTY | CssSyntaxKind::CSS_BOGUS_PROPERTY
        )
    }) else {
        return false;
    };
    let Some(colon) = property
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| t.kind() == CssSyntaxKind::COLON)
    else {
        return false;
    };
    let value_start = usize::from(colon.text_trimmed_range().end());
    if start < value_start {
        return false;
    }

    let value: Vec<_> = property
        .descendants_tokens(Direction::Next)
        .filter(|t| usize::from(t.text_trimmed_range().start()) >= value_start)
        .collect();
    let custom = property
        .first_child()
        .is_some_and(|name| name.kind() == CssSyntaxKind::CSS_DASHED_IDENTIFIER);
    let progid = matches!(
        value.as_slice(),
        [first, second, ..]
            if first.text_trimmed().eq_ignore_ascii_case("progid")
                && second.kind() == CssSyntaxKind::COLON
    );

    (custom || progid) && brackets_close(&value)
}

/// Does every `(`, `[` and `{` among `tokens` meet its own closer, in order?
fn brackets_close(tokens: &[biome_rowan::SyntaxToken<biome_css_syntax::CssLanguage>]) -> bool {
    let mut open: Vec<CssSyntaxKind> = Vec::new();
    for token in tokens {
        match token.kind() {
            CssSyntaxKind::L_PAREN => open.push(CssSyntaxKind::R_PAREN),
            CssSyntaxKind::L_BRACK => open.push(CssSyntaxKind::R_BRACK),
            CssSyntaxKind::L_CURLY => open.push(CssSyntaxKind::R_CURLY),
            kind @ (CssSyntaxKind::R_PAREN | CssSyntaxKind::R_BRACK | CssSyntaxKind::R_CURLY)
                if open.pop() != Some(kind) =>
            {
                return false
            }
            _ => {}
        }
    }
    open.is_empty()
}

/// Is `start..end` a `;` the parser refused as an item of a block?
///
/// CSS Syntax skips a `;` among a block's contents however many stand
/// together. Biome 0.5.8 takes one empty declaration (`a;;`) and raises an
/// error on the next (`a;;;`).
pub fn is_stray_semicolon(ctx: &ParseCtx, start: usize, end: usize) -> bool {
    let Some(node) = covering_node(ctx, start, end) else {
        return false;
    };
    let in_block = node.parent().is_some_and(|p| {
        matches!(
            p.kind(),
            CssSyntaxKind::CSS_DECLARATION_OR_RULE_LIST
                | CssSyntaxKind::CSS_DECLARATION_LIST
                | CssSyntaxKind::CSS_DECLARATION_OR_AT_RULE_LIST
        )
    });
    is_bogus(node.kind())
        && in_block
        && node
            .descendants_tokens(Direction::Next)
            .all(|t| t.kind() == CssSyntaxKind::SEMICOLON)
}

/// Byte offset of the first `}` Biome read into an at-rule's prelude where CSS
/// Syntax ends the at-rule with it: one written last in a block, with no block
/// of its own and no `;`, such as `.a { @apply p-4 }`. That `}` closes the
/// block the at-rule is in; Biome 0.5.8 reads on to the next `;` or `{`, and
/// misreads everything up to there.
///
/// Only for a prelude on one line with its brackets closed and no `:` with a
/// space after it. One that runs onto a new line before the `}`, `@apply p-4`
/// then `color: red`, or holds a `:` the way a declaration does, `@apply p-4
/// gap: 1px`, holds what reads as another statement with its `;` missing, and
/// stays an error. A class name such as `md:p-4` has no space after its `:`.
pub fn at_rule_ended_by_its_block(ctx: &ParseCtx) -> Option<usize> {
    let prelude = ctx
        .syntax()
        .descendants()
        .filter(|n| n.kind() == CssSyntaxKind::CSS_UNKNOWN_AT_RULE_COMPONENT_LIST)
        .find(|n| {
            n.children_with_tokens()
                .any(|e| e.kind() == CssSyntaxKind::R_CURLY)
        })?;
    prelude
        .parent()?
        .parent()
        .filter(|n| n.kind() == CssSyntaxKind::CSS_AT_RULE && is_statement(n))?;
    let tokens: Vec<_> = prelude.descendants_tokens(Direction::Next).collect();
    let close = tokens
        .iter()
        .position(|t| t.kind() == CssSyntaxKind::R_CURLY)?;
    let before = &tokens[..close];
    let one_line = !before.is_empty() && !before.iter().any(|t| t.has_leading_newline());
    let declaration = before.iter().any(|t| {
        t.kind() == CssSyntaxKind::COLON
            && t.trailing_trivia()
                .pieces()
                .any(|piece| piece.is_whitespace())
    });
    (one_line
        && !declaration
        && brackets_close(before)
        && tokens[close].parent().as_ref() == Some(&prelude))
    .then(|| usize::from(tokens[close].text_trimmed_range().start()))
}

/// At-rules that are valid only at the top level of a stylesheet, written
/// inside a block instead, as `(name, start, end)`.
///
/// Biome 0.5.8 parses `.a { @import "x.css"; }` without an error, though CSS
/// Cascade and CSS Nesting make it invalid there, and Tailwind refuses to build
/// a stylesheet that does it.
pub fn misplaced_at_rules(ctx: &ParseCtx) -> Vec<(String, usize, usize)> {
    ctx.syntax()
        .descendants()
        .filter_map(|n| {
            let name = match n.kind() {
                CssSyntaxKind::CSS_IMPORT_AT_RULE => "import",
                CssSyntaxKind::CSS_CHARSET_AT_RULE => "charset",
                CssSyntaxKind::CSS_NAMESPACE_AT_RULE => "namespace",
                _ => return None,
            };
            let at_rule = n.parent()?;
            let top_level = at_rule.parent().is_some_and(|list| {
                list.parent()
                    .is_some_and(|p| p.kind() == CssSyntaxKind::CSS_ROOT)
            });
            if top_level {
                return None;
            }
            let (start, end) = trimmed(&at_rule);
            Some((name.to_string(), start, end))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Statements left unfinished
// ---------------------------------------------------------------------------

/// What a statement in a block is, enough to say what it lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementKind {
    Declaration,
    /// A rule. `property` is the name its selector starts with when that reads
    /// as a declaration with no `:`, which Biome parses as a rule.
    Rule {
        property: Option<String>,
    },
    /// An at-rule, with its name as written.
    AtRule(String),
    Other,
}

/// A declaration, rule or at-rule written in a block, as far as the author
/// wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub kind: StatementKind,
    pub start: usize,
    /// End of its last token before the limit it was read to. A rule ends
    /// before the first `;` in its selector, where the author ended it.
    pub end: usize,
    /// Start of the first token in it before the limit that the lexer could
    /// not read, such as a string with no closing quote.
    pub unread: Option<usize>,
}

/// Is `node` an item of a block: something written between a `{` and its `}`?
fn is_statement(node: &CssSyntaxNode) -> bool {
    node.parent()
        .and_then(|list| list.parent())
        .is_some_and(|block| is_block(&block))
}

/// The name a rule's selector starts with, when nothing but a space or its end
/// follows: `color crimson`, `width 10px`. That is a declaration whose `:` is
/// missing; an `&`, `.a`, `[x]`, `:hover` or `>` against the name makes it a
/// selector.
fn property_name(rule: &CssSyntaxNode) -> Option<String> {
    let name = rule.first_token()?;
    let type_selector = name.parent()?.parent()?;
    let compound = type_selector.parent()?;
    let bare = type_selector.kind() == CssSyntaxKind::CSS_TYPE_SELECTOR
        && compound.text_trimmed_range() == type_selector.text_trimmed_range()
        && compound
            .next_sibling_or_token()
            .is_none_or(|next| next.kind() == CssSyntaxKind::CSS_SPACE_LITERAL);
    bare.then(|| name.text_trimmed().to_string())
}

/// `node`, a statement, read up to `limit`.
fn statement(node: &CssSyntaxNode, limit: usize) -> Statement {
    let kind = match node.kind() {
        CssSyntaxKind::CSS_NESTED_QUALIFIED_RULE | CssSyntaxKind::CSS_QUALIFIED_RULE => {
            StatementKind::Rule {
                property: property_name(node),
            }
        }
        CssSyntaxKind::CSS_AT_RULE => match node.first_token().and_then(|at| at.next_token()) {
            Some(name) if usize::from(name.text_trimmed_range().start()) < limit => {
                StatementKind::AtRule(name.text_trimmed().to_string())
            }
            _ => StatementKind::Other,
        },
        kind if is_declaration_item(kind) => StatementKind::Declaration,
        _ => StatementKind::Other,
    };
    // A rule as written is its selector: Biome reads every token up to the
    // next `{` into it, past any `;`.
    let rule = matches!(kind, StatementKind::Rule { .. });
    let written = if rule { node.first_child() } else { None };
    let start = trimmed(node).0;
    let end = written
        .as_ref()
        .unwrap_or(node)
        .descendants_tokens(Direction::Next)
        .take_while(|t| {
            usize::from(t.text_trimmed_range().start()) < limit
                && !(rule && t.kind() == CssSyntaxKind::SEMICOLON)
        })
        .filter(|t| t.kind() != CssSyntaxKind::CSS_SPACE_LITERAL)
        .last()
        .map_or(start, |t| usize::from(t.text_trimmed_range().end()));
    let unread = node
        .descendants_tokens(Direction::Next)
        .take_while(|t| usize::from(t.text_trimmed_range().start()) < limit)
        .find(|t| t.kind() == CssSyntaxKind::ERROR_TOKEN)
        .map(|t| usize::from(t.text_trimmed_range().start()));
    Statement {
        kind,
        start,
        end,
        unread,
    }
}

/// Nested rules whose selector cannot be one: it holds a `;`, or no `{`
/// follows it. That is how Biome reads a declaration with no `:`, such as
/// `color crimson;`, together with everything after it up to the next `{`, or
/// to the end of the text when there is none. Each comes with the offset
/// where that reading stops, and is read as written up to `limit`.
///
/// A token the lexer could not read, such as a string with no closing quote,
/// is what Biome misread the rest from: a rule with one in what the author
/// wrote is not listed, and one after its `;` stops the reading there.
pub fn misread_rules(ctx: &ParseCtx, limit: usize) -> Vec<(Statement, usize)> {
    ctx.syntax()
        .descendants()
        .filter(|n| n.kind() == CssSyntaxKind::CSS_NESTED_QUALIFIED_RULE)
        .filter_map(|rule| {
            let block = block_child(&rule).map(|block| trimmed(&block).0);
            let semicolon = rule
                .first_child()?
                .descendants_tokens(Direction::Next)
                .any(|t| t.kind() == CssSyntaxKind::SEMICOLON);
            if !semicolon && block.is_some() {
                return None;
            }
            let read = statement(&rule, limit);
            if read.unread.is_some_and(|at| at < read.end) {
                return None;
            }
            let reach = block
                .unwrap_or(usize::MAX)
                .min(read.unread.unwrap_or(usize::MAX));
            Some((read, reach))
        })
        .collect()
}

/// Byte offset of the `{` whose block the `}` at `at` closes. `None` when no
/// `}` token starts there, or Biome read that one into a selector, a value or
/// an at-rule's prelude instead.
pub fn opening_brace(ctx: &ParseCtx, at: usize) -> Option<usize> {
    let offset = TextSize::from(u32::try_from(at).ok()?);
    if !ctx.syntax().text_range().contains(offset) {
        return None;
    }
    let close = ctx.syntax().token_at_offset(offset).right_biased()?;
    if close.kind() != CssSyntaxKind::R_CURLY
        || usize::from(close.text_trimmed_range().start()) != at
    {
        return None;
    }
    let block = close.parent().filter(is_block)?;
    block
        .first_token()
        .map(|open| usize::from(open.text_trimmed_range().start()))
}

/// The innermost statement holding the last token that starts before
/// `limit`, read up to `limit`.
pub fn statement_before(ctx: &ParseCtx, limit: usize) -> Option<Statement> {
    let offset = TextSize::from(u32::try_from(limit.checked_sub(1)?).ok()?);
    if !ctx.syntax().text_range().contains(offset) {
        return None;
    }
    let mut last = ctx.syntax().token_at_offset(offset).left_biased()?;
    while usize::from(last.text_trimmed_range().start()) >= limit {
        last = last.prev_token()?;
    }
    let node = last.ancestors().find(is_statement)?;
    Some(statement(&node, limit))
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
    fn a_plus_inside_a_functional_pseudo_is_not_a_combinator() {
        // Both spellings denote the same selector, and reading the CST rather
        // than the text makes them agree, so either spelling matches the
        // other.
        assert_eq!(
            normalize_selector("li:nth-child(2n + 1)"),
            "li:nth-child(2n+1)"
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

#[cfg(test)]
mod nesting_tests {
    use super::*;
    use crate::ctx::ParseCtx;

    fn ctx(src: &str) -> ParseCtx {
        ParseCtx::parse_default(src)
    }

    const NESTED: &str = ".card {\n  color: red;\n  &:hover {\n    color: blue;\n  }\n  .title {\n    font-size: 2rem;\n  }\n}\n";

    #[test]
    fn nested_rules_are_found_when_descending() {
        let c = ctx(NESTED);
        let all: Vec<String> = find_all_rules(&c)
            .into_iter()
            .map(|r| r.selector_norm)
            .collect();
        assert_eq!(all, vec![".card", "&:hover", ".title"]);
    }

    #[test]
    fn nested_rules_stay_out_of_the_top_level() {
        let c = ctx(NESTED);
        let top: Vec<String> = find_top_level_rules(&c)
            .into_iter()
            .map(|r| r.selector_norm)
            .collect();
        assert_eq!(top, vec![".card"]);
        assert!(find_rule_by_selector(&c, "&:hover").is_none());
    }

    #[test]
    fn a_nested_rule_can_be_addressed_explicitly() {
        let c = ctx(NESTED);
        assert!(matches!(
            find_rule_by_selector_anywhere(&c, "&:hover"),
            MatchResult::One(_)
        ));
        assert!(matches!(
            find_rule_by_selector_anywhere(&c, ".title"),
            MatchResult::One(_)
        ));
    }

    #[test]
    fn declarations_of_a_nested_rule_are_its_own() {
        let c = ctx(NESTED);
        let hover = find_rule_by_selector_anywhere(&c, "&:hover").one().unwrap();
        let decls: Vec<String> = declarations_in(&c, &hover)
            .into_iter()
            .map(|d| d.property)
            .collect();
        assert_eq!(decls, vec!["color"]);
    }

    #[test]
    fn an_outer_rule_lists_only_its_own_declarations() {
        let c = ctx(NESTED);
        let card = find_rule_by_selector(&c, ".card").one().unwrap();
        let decls: Vec<String> = declarations_in(&c, &card)
            .into_iter()
            .map(|d| d.property)
            .collect();
        assert_eq!(decls, vec!["color"]);
    }

    #[test]
    fn nesting_inside_an_at_rule_is_reached_too() {
        let c =
            ctx("@media print {\n  .a {\n    color: red;\n    &:hover { color: blue; }\n  }\n}\n");
        let all: Vec<String> = find_all_rules(&c)
            .into_iter()
            .map(|r| r.selector_norm)
            .collect();
        assert_eq!(all, vec![".a", "&:hover"]);
    }
}
