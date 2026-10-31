// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Read-only analysis. Nothing in this module produces an edit, so none of it
//! can violate the diff-minimality constraint -- these are the queries the
//! Elixir side uses to report on a stylesheet.

use crate::ctx::{ParseCtx, ParseError, ParseOptions};
use crate::error::Result;
use crate::locate::{
    all_comments, at_rule_body, closing_brace, declaration_lists, declarations_in_block,
    find_all_at_rules, find_all_rules, find_at_rules_named, find_top_level_rules,
    in_free_form_value, is_stray_semicolon, misplaced_at_rules, DeclRef,
};
use crate::ops::at_rule::normalize_target_needle;
use crate::ops::query;
use biome_css_syntax::{CssSyntaxKind, CssSyntaxNode};
use biome_rowan::Direction;
use std::collections::{BTreeMap, BTreeSet};

/// A block of declarations together with the text that introduced it -- a
/// selector, a keyframe step, or an at-rule prelude.
#[derive(Debug, Clone)]
pub struct Block {
    pub prelude: String,
    /// `@media`/`@supports`/`@container` conditions enclosing this block,
    /// outermost first.
    pub conditions: Vec<String>,
    pub declarations: Vec<(String, String)>,
}

fn declaration_pairs(decls: &[DeclRef]) -> Vec<(String, String)> {
    decls
        .iter()
        .map(|d| {
            let value = if d.important {
                format!("{} !important", d.value_raw.trim())
            } else {
                d.value_raw.trim().to_string()
            };
            (d.property.clone(), value)
        })
        .collect()
}

/// The text that introduces the block containing `list`: everything from the
/// start of the enclosing node up to its `{`.
fn prelude_of(ctx: &ParseCtx, list: &CssSyntaxNode) -> String {
    let Some(block) = list.parent() else {
        return String::new();
    };
    let Some(owner) = block.parent() else {
        return String::new();
    };
    let start = usize::from(owner.text_trimmed_range().start());
    let brace = usize::from(block.text_trimmed_range().start());
    ctx.source()
        .get(start..brace)
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `@media`/`@supports`/`@container` preludes enclosing this node, outermost
/// first.
fn conditions_of(ctx: &ParseCtx, node: &CssSyntaxNode) -> Vec<String> {
    // Read each ancestor directly. Searching a freshly built list of every
    // at-rule in the file, once per ancestor, made this quadratic in nesting
    // depth for a node we already hold.
    let mut out: Vec<String> = node
        .ancestors()
        .filter(|a| a.kind() == CssSyntaxKind::CSS_AT_RULE)
        .filter_map(|a| {
            crate::locate::at_rule_ref(ctx, &a)
                .filter(|r| matches!(r.name.as_str(), "media" | "supports" | "container"))
                .map(|r| format!("@{} {}", r.name, r.prelude).trim().to_string())
        })
        .collect();
    out.reverse();
    out
}

/// Every declaration-bearing block in the file.
pub fn blocks(ctx: &ParseCtx) -> Vec<Block> {
    declaration_lists(ctx)
        .into_iter()
        .filter(|(_, d)| !d.is_empty())
        .map(|(list, decls)| Block {
            prelude: prelude_of(ctx, &list),
            conditions: conditions_of(ctx, &list),
            declarations: declaration_pairs(&decls),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------

/// Functions that produce a colour. Names, because the CST models them all as
/// `CSS_FUNCTION` -- the spec list is data, not a parsing shortcut.
const COLOR_FUNCTIONS: &[&str] = &[
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "hwb",
    "lab",
    "lch",
    "oklab",
    "oklch",
    "color",
    "color-mix",
    "light-dark",
];

/// CSS named colours. Matched against `CSS_IDENTIFIER` tokens only, never
/// against arbitrary text, so a path like `url(/red.png)` cannot be mistaken
/// for one -- Biome parses that as `CSS_URL_FUNCTION` with a raw value, which
/// is structurally not an identifier.
const NAMED_COLORS: &[&str] = &[
    "aliceblue",
    "antiquewhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "black",
    "blanchedalmond",
    "blue",
    "blueviolet",
    "brown",
    "burlywood",
    "cadetblue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "crimson",
    "currentcolor",
    "cyan",
    "darkblue",
    "darkcyan",
    "darkgoldenrod",
    "darkgray",
    "darkgreen",
    "darkgrey",
    "darkkhaki",
    "darkmagenta",
    "darkolivegreen",
    "darkorange",
    "darkorchid",
    "darkred",
    "darksalmon",
    "darkseagreen",
    "darkslateblue",
    "darkslategray",
    "darkslategrey",
    "darkturquoise",
    "darkviolet",
    "deeppink",
    "deepskyblue",
    "dimgray",
    "dimgrey",
    "dodgerblue",
    "firebrick",
    "floralwhite",
    "forestgreen",
    "fuchsia",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "gray",
    "green",
    "greenyellow",
    "grey",
    "honeydew",
    "hotpink",
    "indianred",
    "indigo",
    "ivory",
    "khaki",
    "lavender",
    "lavenderblush",
    "lawngreen",
    "lemonchiffon",
    "lightblue",
    "lightcoral",
    "lightcyan",
    "lightgoldenrodyellow",
    "lightgray",
    "lightgreen",
    "lightgrey",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightslategrey",
    "lightsteelblue",
    "lightyellow",
    "lime",
    "limegreen",
    "linen",
    "magenta",
    "maroon",
    "mediumaquamarine",
    "mediumblue",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumslateblue",
    "mediumspringgreen",
    "mediumturquoise",
    "mediumvioletred",
    "midnightblue",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "navy",
    "oldlace",
    "olive",
    "olivedrab",
    "orange",
    "orangered",
    "orchid",
    "palegoldenrod",
    "palegreen",
    "paleturquoise",
    "palevioletred",
    "papayawhip",
    "peachpuff",
    "peru",
    "pink",
    "plum",
    "powderblue",
    "purple",
    "rebeccapurple",
    "red",
    "rosybrown",
    "royalblue",
    "saddlebrown",
    "salmon",
    "sandybrown",
    "seagreen",
    "seashell",
    "sienna",
    "silver",
    "skyblue",
    "slateblue",
    "slategray",
    "slategrey",
    "snow",
    "springgreen",
    "steelblue",
    "tan",
    "teal",
    "thistle",
    "tomato",
    "transparent",
    "turquoise",
    "violet",
    "wheat",
    "white",
    "whitesmoke",
    "yellow",
    "yellowgreen",
];

/// A `CSS_COLOR_LITERAL` carries the digits without the `#`. Biome parses
/// `#notahex` as a colour node too, so the literal still has to be checked
/// against the spec's permitted lengths.
fn is_hex_literal(text: &str) -> bool {
    matches!(text.len(), 3 | 4 | 6 | 8) && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Does this declaration's value contain a colour?
///
/// Decided from the CST, not from the text. Biome already distinguishes the
/// three cases: a hex colour is a `CSS_COLOR` node, `rgb(...)` and friends are
/// `CSS_FUNCTION` nodes carrying an identifier name, and a bare `red` is a
/// `CSS_IDENTIFIER`. A `url(...)` payload is a `CSS_URL_FUNCTION` with a raw
/// value node, so its contents are never identifiers and cannot be misread.
pub fn value_node_has_color(value: &CssSyntaxNode) -> bool {
    value.descendants().any(|node| match node.kind() {
        // `#fff`, `#ffffffcc` -- shape from the CST, digits from the spec.
        CssSyntaxKind::CSS_COLOR => node.descendants_tokens(Direction::Next).any(|t| {
            t.kind() == CssSyntaxKind::CSS_COLOR_LITERAL && is_hex_literal(t.text_trimmed())
        }),
        // `rgb(...)`, `oklch(...)`, `color-mix(...)`
        CssSyntaxKind::CSS_FUNCTION => node
            .first_token()
            .is_some_and(|t| COLOR_FUNCTIONS.contains(&t.text_trimmed().to_lowercase().as_str())),
        // `red`, `transparent`, `currentColor`
        CssSyntaxKind::CSS_IDENTIFIER => node
            .first_token()
            .is_some_and(|t| NAMED_COLORS.contains(&t.text_trimmed().to_lowercase().as_str())),
        _ => false,
    })
}

/// String-level convenience for callers that only have the value text. Parses
/// it so the answer comes from the same CST walk as everything else.
pub fn value_has_color(value: &str) -> bool {
    if value.trim().is_empty() {
        return false;
    }
    if crate::ctx::check_nesting(value).is_err() {
        return false;
    }
    let probe = format!("a{{b:{value}}}");
    let Ok(ctx) = ParseCtx::try_new(&probe, ParseOptions::default()) else {
        return false;
    };
    ctx.syntax()
        .descendants()
        .find(|n| n.kind() == CssSyntaxKind::CSS_GENERIC_COMPONENT_VALUE_LIST)
        .is_some_and(|list| value_node_has_color(&list))
}

/// Colour-carrying declarations, grouped by the selector they belong to.
pub fn extract_colors(source: &str, options: ParseOptions) -> Result<Vec<(String, Vec<String>)>> {
    query(source, options, |ctx| {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for block in blocks(ctx) {
            let hits: Vec<String> = block
                .declarations
                .iter()
                .filter(|(_, v)| value_has_color(v))
                .map(|(p, v)| format!("{p}: {v}"))
                .collect();
            if hits.is_empty() {
                continue;
            }
            match out.iter_mut().find(|(sel, _)| *sel == block.prelude) {
                Some((_, list)) => list.extend(hits),
                None => out.push((block.prelude.clone(), hits)),
            }
        }
        Ok(out)
    })
}

// ---------------------------------------------------------------------------
// Media queries
// ---------------------------------------------------------------------------

/// One rule inside a media query.
pub type MediaRule = (String, Vec<(String, String)>);

/// Media queries in the file, each mapped to the rules it contains.
pub fn extract_media_queries(
    source: &str,
    options: ParseOptions,
) -> Result<Vec<(String, Vec<MediaRule>)>> {
    query(source, options, |ctx| {
        let mut out: Vec<(String, Vec<MediaRule>)> = Vec::new();
        for at in find_all_at_rules(ctx) {
            if at.name != "media" || !at.has_block {
                continue;
            }
            let key = at.prelude.split_whitespace().collect::<Vec<_>>().join(" ");
            let (Some(open), Some(close)) = (at.body_open, at.body_close) else {
                continue;
            };
            let rules: Vec<MediaRule> = find_all_rules(ctx)
                .into_iter()
                .filter(|r| r.start >= open && r.end <= close)
                .map(|r| {
                    let decls = crate::locate::declarations_in(ctx, &r);
                    (r.selector_raw.clone(), declaration_pairs(&decls))
                })
                .collect();
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some((_, list)) => list.extend(rules),
                None => out.push((key, rules)),
            }
        }
        Ok(out)
    })
}

// ---------------------------------------------------------------------------
// Animations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Animation {
    pub name: String,
    /// `("0%", [("opacity", "0")])` in source order.
    pub keyframes: Vec<(String, Vec<(String, String)>)>,
    /// Selectors whose `animation` / `animation-name` mentions this animation.
    pub used_by: Vec<String>,
}

/// Does `value` reference the animation `name` as a whole token?
fn references_animation(value: &str, name: &str) -> bool {
    value
        .split(|c: char| c.is_whitespace() || c == ',')
        .any(|t| t.trim() == name)
}

pub fn extract_animations(source: &str, options: ParseOptions) -> Result<Vec<Animation>> {
    query(source, options, |ctx| {
        let all_blocks = blocks(ctx);
        let mut out = Vec::new();

        for at in find_all_at_rules(ctx) {
            if !at.name.ends_with("keyframes") || !at.has_block {
                continue;
            }
            let name = at.prelude.trim().trim_matches(['"', '\'']).to_string();
            let (Some(open), Some(close)) = (at.body_open, at.body_close) else {
                continue;
            };

            let keyframes: Vec<(String, Vec<(String, String)>)> = declaration_lists(ctx)
                .into_iter()
                .filter(|(list, _)| {
                    let s = usize::from(list.text_trimmed_range().start());
                    s >= open && s <= close
                })
                .map(|(list, decls)| (prelude_of(ctx, &list), declaration_pairs(&decls)))
                .collect();

            let used_by: Vec<String> = all_blocks
                .iter()
                .filter(|b| {
                    b.declarations.iter().any(|(p, v)| {
                        matches!(p.to_lowercase().as_str(), "animation" | "animation-name")
                            && references_animation(v, &name)
                    })
                })
                .map(|b| b.prelude.clone())
                .collect();

            out.push(Animation {
                name,
                keyframes,
                used_by,
            });
        }
        Ok(out)
    })
}

// ---------------------------------------------------------------------------
// At-rules, read back whole
// ---------------------------------------------------------------------------

/// One at-rule, with the parts a caller needs to read a decision out of it.
///
/// `analyze` only reports that a `@plugin` exists; this reports *which* plugin
/// and how it was configured, which is what an installer needs before it can
/// generate code that agrees with the user's setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtRule {
    /// Lowercased, without the `@`.
    pub name: String,
    /// Everything between the name and the `;` or `{`, trimmed.
    pub prelude: String,
    /// The first string literal or `url()` value before any block, unquoted --
    /// `"daisyui"` in `@plugin "daisyui" { … }`.
    pub target: Option<String>,
    pub has_block: bool,
    /// Declarations inside the block, in source order. Empty when the at-rule
    /// has no block, or a block that holds rules rather than declarations.
    pub declarations: Vec<(String, String)>,
    /// The at-rule's own bytes, exactly as written.
    pub text: String,
    /// The bytes between the block's braces, exactly as written. `None` for an
    /// at-rule with no block.
    pub body: Option<String>,
}

/// Every **top-level** at-rule named `name`, optionally narrowed to those
/// `matching` names -- see [`crate::locate::AtRuleRef::matches`].
///
/// Top-level only, for the same reason the codemods are: a `@plugin` nested
/// inside a `@layer` is a different thing from one at the file's root, and
/// guessing between them is how a caller gets a surprising answer.
pub fn get_at_rules(
    source: &str,
    name: &str,
    matching: Option<&str>,
    options: ParseOptions,
) -> Result<Vec<AtRule>> {
    let wanted = name.trim().trim_start_matches('@').to_lowercase();
    let want = matching.map(normalize_target_needle);

    query(source, options, |ctx| {
        Ok(find_at_rules_named(ctx, &wanted)
            .iter()
            .filter(|at| at.matches(want.as_deref()))
            .map(|at| AtRule {
                name: at.name.clone(),
                prelude: at.prelude.clone(),
                target: at.target.clone(),
                has_block: at.has_block,
                declarations: at_rule_body(at)
                    .map(|block| {
                        declarations_in_block(ctx, &block)
                            .iter()
                            .map(|d| (d.property.clone(), d.value_raw.trim().to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                text: ctx.source()[at.start..at.end].to_string(),
                body: match (at.body_open, at.body_close) {
                    (Some(open), Some(close)) => Some(ctx.source()[open..close].to_string()),
                    _ => None,
                },
            })
            .collect())
    })
}

// ---------------------------------------------------------------------------
// Stylesheet statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Analysis {
    pub rules_count: usize,
    pub top_level_rules_count: usize,
    pub selectors_count: usize,
    pub unique_selectors: usize,
    pub declarations_count: usize,
    pub unique_properties: usize,
    pub at_rules_count: usize,
    pub media_queries_count: usize,
    pub keyframes_count: usize,
    pub imports_count: usize,
    pub comments_count: usize,
    pub colors_count: usize,
    pub important_count: usize,
    pub custom_properties_count: usize,
    /// Properties by descending frequency, then name.
    pub property_frequency: Vec<(String, usize)>,
    pub selectors: Vec<String>,
    pub at_rule_names: Vec<String>,
}

pub fn analyze(source: &str, options: ParseOptions) -> Result<Analysis> {
    query(source, options, |ctx| {
        let rules = find_all_rules(ctx);
        let at_rules = find_all_at_rules(ctx);

        // A selector list counts once per comma-separated selector.
        let mut selectors: Vec<String> = Vec::new();
        for r in &rules {
            for part in r.selector_norm.split(',') {
                let part = part.trim();
                if !part.is_empty() {
                    selectors.push(part.to_string());
                }
            }
        }
        let unique_selectors: BTreeSet<&String> = selectors.iter().collect();

        let mut frequency: BTreeMap<String, usize> = BTreeMap::new();
        let mut declarations_count = 0usize;
        let mut colors_count = 0usize;
        let mut important_count = 0usize;
        let mut custom_properties_count = 0usize;

        for (_, decls) in declaration_lists(ctx) {
            for d in &decls {
                declarations_count += 1;
                *frequency
                    .entry(crate::locate::normalize_property(&d.property))
                    .or_insert(0) += 1;
                if d.important {
                    important_count += 1;
                }
                if d.property.starts_with("--") {
                    custom_properties_count += 1;
                }
                if value_has_color(&d.value_raw) {
                    colors_count += 1;
                }
            }
        }

        let mut property_frequency: Vec<(String, usize)> =
            frequency.iter().map(|(k, v)| (k.clone(), *v)).collect();
        property_frequency.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

        let mut at_rule_names: Vec<String> = at_rules
            .iter()
            .map(|r| r.name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        at_rule_names.sort();

        Ok(Analysis {
            rules_count: rules.len(),
            top_level_rules_count: find_top_level_rules(ctx).len(),
            selectors_count: selectors.len(),
            unique_selectors: unique_selectors.len(),
            declarations_count,
            unique_properties: frequency.len(),
            at_rules_count: at_rules.len(),
            media_queries_count: at_rules.iter().filter(|r| r.name == "media").count(),
            keyframes_count: at_rules
                .iter()
                .filter(|r| r.name.ends_with("keyframes"))
                .count(),
            imports_count: at_rules.iter().filter(|r| r.name == "import").count(),
            comments_count: all_comments(ctx).len(),
            colors_count,
            important_count,
            custom_properties_count,
            property_frequency,
            selectors: find_top_level_rules(ctx)
                .into_iter()
                .map(|r| r.selector_raw)
                .collect(),
            at_rule_names,
        })
    })
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validation {
    pub valid: bool,
    /// How many diagnostics the parser raised, before any of them is set aside
    /// as an error it raises against valid CSS. `errors` is the verdict.
    pub diagnostics: usize,
    pub round_trips: bool,
    pub message: String,
    pub errors: Vec<Diagnostic>,
}

/// One reason a stylesheet or a block is not valid, placed in the text the
/// caller handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    /// 1-based.
    pub line: usize,
    /// 1-based, counted in characters from the start of the line.
    pub column: usize,
    /// Byte offset of the start of `text`.
    pub offset: usize,
    /// The text the error is about; empty when it is about the end of the text.
    pub text: String,
}

impl Validation {
    fn refused(message: String) -> Self {
        Validation {
            valid: false,
            diagnostics: 0,
            round_trips: false,
            errors: vec![Diagnostic {
                message: message.clone(),
                line: 1,
                column: 1,
                offset: 0,
                text: String::new(),
            }],
            message,
        }
    }

    fn of(ctx: &ParseCtx, errors: Vec<Diagnostic>) -> Self {
        let round_trips = ctx.round_trips();
        let message = if !round_trips {
            "the parser did not reproduce the input byte for byte; refusing to patch this file"
                .to_string()
        } else {
            match errors.as_slice() {
                [] => "CSS is valid".to_string(),
                [first, ..] => format!(
                    "CSS parsed with {} diagnostic(s); the first is at line {}, column {}: {}",
                    errors.len(),
                    first.line,
                    first.column,
                    first.message
                ),
            }
        };
        Validation {
            valid: round_trips && errors.is_empty(),
            diagnostics: ctx.diagnostics_count(),
            round_trips,
            message,
            errors,
        }
    }
}

/// What is wrong with a parse, in source order: the parser's own errors less
/// the ones it raises against valid CSS, and the at-rules written inside a
/// block that are valid only at the top level of a stylesheet.
fn errors(ctx: &ParseCtx) -> Vec<ParseError> {
    let mut errors: Vec<ParseError> = ctx
        .errors()
        .into_iter()
        .filter(|e| {
            !in_free_form_value(ctx, e.start, e.end) && !is_stray_semicolon(ctx, e.start, e.end)
        })
        .collect();
    errors.extend(
        misplaced_at_rules(ctx)
            .into_iter()
            .map(|(name, start, end)| ParseError {
                message: format!("`@{name}` is valid only at the top level of a stylesheet"),
                start,
                end,
            }),
    );
    errors.sort_by_key(|e| (e.start, e.end));
    errors
}

/// `error` placed in `text`, whose first byte is at `shift` in the parsed
/// source. A range that reaches past either end of `text` is clamped to it.
fn placed(text: &str, shift: usize, error: &ParseError) -> Diagnostic {
    let clamp = |at: usize| {
        let mut at = at.saturating_sub(shift).min(text.len());
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    let start = clamp(error.start);
    let end = clamp(error.end).max(start);
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);

    Diagnostic {
        message: error.message.clone(),
        line: text[..start].matches('\n').count() + 1,
        column: text[line_start..start].chars().count() + 1,
        offset: start,
        text: text[start..end].to_string(),
    }
}

/// Is this CSS understood well enough to patch?
///
/// "Valid" here means: the parser reproduced the input byte for byte **and**
/// raised no errors. The round-trip half is the one that actually matters for
/// safety -- it is what every codemod checks before touching a file.
///
/// The errors are the parser's, less the ones Biome 0.5.8 raises against valid
/// CSS (see `in_free_form_value` and `is_stray_semicolon`), plus an `@import`,
/// `@charset` or `@namespace` written inside a block, which it does not raise.
pub fn validate(source: &str, options: ParseOptions) -> Validation {
    if let Err(e) = crate::ctx::check_nesting(source) {
        return Validation::refused(e.to_string());
    }
    let ctx = match ParseCtx::try_new(source, options) {
        Ok(c) => c,
        Err(e) => return Validation::refused(e.to_string()),
    };
    // Offsets are into the source without its byte-order mark, which no editor
    // shows as a column.
    let bom = if ctx.has_bom() {
        crate::ctx::BOM.len()
    } else {
        0
    };
    let errors = errors(&ctx)
        .iter()
        .map(|e| {
            let mut d = placed(ctx.source(), 0, e);
            d.offset += bom;
            d
        })
        .collect();
    Validation::of(&ctx, errors)
}

/// The rule a block is read inside, its body on lines of its own: a body that
/// ends in a `//` comment, where those are read as comments, cannot then
/// comment out the closing brace.
const BLOCK_OPEN: &str = ".block {\n";
const BLOCK_CLOSE: &str = "\n}\n";

/// Is `body` valid as the contents of one rule's block: declarations, nested
/// rules and at-rules, what an editor gives a single element?
///
/// Read as the parser reads a block, by putting it inside one, with every
/// error placed in `body` itself. On top of `validate/2`'s checks, a `}` in
/// `body` that closes the block it was put in is an error: everything after it
/// would be outside that block, at the top level of whatever stylesheet the
/// body is written into, and still parse.
pub fn validate_block(body: &str, options: ParseOptions) -> Validation {
    if let Err(e) = crate::ctx::check_nesting(body) {
        return Validation::refused(e.to_string());
    }
    let source = format!("{BLOCK_OPEN}{body}{BLOCK_CLOSE}");
    let ctx = match ParseCtx::try_new(&source, options) {
        Ok(c) => c,
        Err(e) => return Validation::refused(e.to_string()),
    };

    let mut found = errors(&ctx);
    let body_end = BLOCK_OPEN.len() + body.len();
    let closed_early = find_top_level_rules(&ctx)
        .first()
        .and_then(closing_brace)
        .filter(|&at| at < body_end);
    if let Some(at) = closed_early {
        found.push(ParseError {
            message: "this `}` closes the block it is written in; everything after it is outside"
                .to_string(),
            start: at,
            end: at + 1,
        });
        found.sort_by_key(|e| (e.start, e.end));
    }

    let errors = found
        .iter()
        .map(|e| placed(body, BLOCK_OPEN.len(), e))
        .collect();
    Validation::of(&ctx, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> ParseOptions {
        ParseOptions::default()
    }

    // -- at-rules -----------------------------------------------------------

    const TAILWIND: &str = r#"@import "tailwindcss";
@plugin "../vendor/heroicons";
@plugin "daisyui" {
  prefix: "d-"; /* keeps daisyUI off our own .btn */
  exclude: rootcolor;
  logs: false;
}
@source "../js";

.btn { color: red; }
"#;

    #[test]
    fn reads_an_at_rule_block_as_declarations() {
        let found = get_at_rules(TAILWIND, "plugin", Some("daisyui"), opts()).unwrap();
        assert_eq!(found.len(), 1);

        let at = &found[0];
        assert_eq!(at.name, "plugin");
        assert_eq!(at.target.as_deref(), Some("daisyui"));
        assert!(at.has_block);
        assert_eq!(
            at.declarations,
            vec![
                ("prefix".to_string(), "\"d-\"".to_string()),
                ("exclude".to_string(), "rootcolor".to_string()),
                ("logs".to_string(), "false".to_string()),
            ]
        );
    }

    #[test]
    fn narrows_by_target_and_returns_every_match_without_one() {
        assert_eq!(
            get_at_rules(TAILWIND, "plugin", None, opts())
                .unwrap()
                .len(),
            2
        );
        assert!(get_at_rules(TAILWIND, "plugin", Some("nope"), opts())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn matching_reads_a_target_however_it_is_quoted() {
        assert_eq!(
            get_at_rules(TAILWIND, "plugin", Some("\"daisyui\""), opts()).unwrap(),
            get_at_rules(TAILWIND, "plugin", Some("daisyui"), opts()).unwrap()
        );
    }

    #[test]
    fn matching_a_targetless_at_rule_reads_its_prelude() {
        let src = "@theme inline {\n  --font: x;\n}\n@theme {\n  --a: 1;\n}\n";
        let plain = get_at_rules(src, "theme", Some(""), opts()).unwrap();
        assert_eq!(plain.len(), 1);
        assert_eq!(
            plain[0].declarations,
            vec![("--a".to_string(), "1".to_string())]
        );
        let inline = get_at_rules(src, "theme", Some("inline"), opts()).unwrap();
        assert_eq!(inline.len(), 1);
        assert_eq!(inline[0].prelude, "inline");
    }

    #[test]
    fn a_blockless_at_rule_reports_no_declarations() {
        let found = get_at_rules(TAILWIND, "import", None, opts()).unwrap();
        assert_eq!(found.len(), 1);
        assert!(!found[0].has_block);
        assert!(found[0].declarations.is_empty());
        assert_eq!(found[0].target.as_deref(), Some("tailwindcss"));
    }

    #[test]
    fn the_leading_at_is_optional_in_the_name() {
        assert_eq!(
            get_at_rules(TAILWIND, "@source", None, opts()).unwrap(),
            get_at_rules(TAILWIND, "source", None, opts()).unwrap()
        );
    }

    #[test]
    fn reports_the_at_rule_text_verbatim_comments_included() {
        let found = get_at_rules(TAILWIND, "plugin", Some("daisyui"), opts()).unwrap();
        assert!(found[0].text.starts_with("@plugin \"daisyui\" {"));
        assert!(found[0]
            .text
            .contains("/* keeps daisyUI off our own .btn */"));
    }

    #[test]
    fn an_absent_at_rule_is_an_empty_list_not_an_error() {
        assert!(get_at_rules(".a { color: red; }\n", "plugin", None, opts())
            .unwrap()
            .is_empty());
    }

    // -- colours ------------------------------------------------------------

    #[test]
    fn recognises_colour_values() {
        for v in [
            "#fff",
            "#ffffff",
            "#ffffffcc",
            "rgb(0 0 0)",
            "rgba(0,0,0,.5)",
            "hsl(1 2% 3%)",
            "oklch(0.99 0 0)",
            "red",
            "rebeccapurple",
            "transparent",
            "currentColor",
            "color-mix(in oklab, red, blue)",
            "1px solid #333",
        ] {
            assert!(value_has_color(v), "should be a colour: {v}");
        }
    }

    #[test]
    fn does_not_mistake_other_values_for_colours() {
        for v in [
            "0",
            "1rem",
            "none",
            "flex",
            "var(--brand)",
            "#notahex",
            "url(/red.png)",
            "\"redacted\"",
            "translate(10px)",
        ] {
            assert!(!value_has_color(v), "should not be a colour: {v}");
        }
    }

    #[test]
    fn extracts_colours_by_selector() {
        let src = ".a {\n  color: #333;\n  margin: 0;\n}\n.b {\n  background: rgba(0,0,0,.5);\n}\n";
        let out = extract_colors(src, opts()).unwrap();
        assert_eq!(
            out,
            vec![
                (".a".to_string(), vec!["color: #333".to_string()]),
                (
                    ".b".to_string(),
                    vec!["background: rgba(0,0,0,.5)".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn colour_extraction_reaches_into_media_blocks() {
        let src = "@media print {\n  .a { color: red; }\n}\n";
        let out = extract_colors(src, opts()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, ".a");
    }

    #[test]
    fn colour_extraction_of_an_empty_sheet_is_empty() {
        assert!(extract_colors("", opts()).unwrap().is_empty());
    }

    // -- media queries ------------------------------------------------------

    #[test]
    fn extracts_media_queries_with_their_rules() {
        let src = "@media (max-width: 768px) {\n  .a {\n    font-size: 14px;\n  }\n}\n";
        let out = extract_media_queries(src, opts()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "(max-width: 768px)");
        assert_eq!(
            out[0].1,
            vec![(".a".to_string(), vec![("font-size".into(), "14px".into())])]
        );
    }

    #[test]
    fn merges_repeated_media_queries() {
        let src = "@media print {\n  .a {}\n}\n@media print {\n  .b {}\n}\n";
        let out = extract_media_queries(src, opts()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1.len(), 2);
    }

    #[test]
    fn a_sheet_without_media_queries_yields_nothing() {
        assert!(extract_media_queries(".a {}\n", opts()).unwrap().is_empty());
    }

    // -- animations ---------------------------------------------------------

    #[test]
    fn extracts_keyframes_and_their_users() {
        let src = "@keyframes fade-in {\n  from {\n    opacity: 0;\n  }\n  to {\n    opacity: 1;\n  }\n}\n.a {\n  animation: fade-in 1s;\n}\n";
        let out = extract_animations(src, opts()).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "fade-in");
        assert_eq!(
            out[0].keyframes,
            vec![
                ("from".to_string(), vec![("opacity".into(), "0".into())]),
                ("to".to_string(), vec![("opacity".into(), "1".into())]),
            ]
        );
        assert_eq!(out[0].used_by, vec![".a".to_string()]);
    }

    #[test]
    fn animation_name_matching_is_token_exact() {
        let src = "@keyframes slide {\n  0% { left: 0; }\n}\n.a { animation: slide-in 1s; }\n.b { animation-name: slide; }\n";
        let out = extract_animations(src, opts()).unwrap();
        assert_eq!(out[0].used_by, vec![".b".to_string()]);
    }

    #[test]
    fn an_unused_animation_reports_no_users() {
        let src = "@keyframes x {\n  0% { left: 0; }\n}\n";
        let out = extract_animations(src, opts()).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].used_by.is_empty());
    }

    #[test]
    fn a_sheet_without_keyframes_yields_nothing() {
        assert!(extract_animations(".a {}\n", opts()).unwrap().is_empty());
    }

    // -- statistics ---------------------------------------------------------

    #[test]
    fn counts_the_basics() {
        let src = "/* c */\n@import \"x\";\n.a, .b {\n  color: red;\n  margin: 0 !important;\n}\n#c {\n  --x: 1;\n}\n@media print {\n  .d { color: blue; }\n}\n";
        let a = analyze(src, opts()).unwrap();
        assert_eq!(a.rules_count, 3);
        assert_eq!(a.top_level_rules_count, 2);
        assert_eq!(a.selectors_count, 4);
        assert_eq!(a.unique_selectors, 4);
        assert_eq!(a.declarations_count, 4);
        // color, margin, --x -- `color` appears twice.
        assert_eq!(a.unique_properties, 3);
        assert_eq!(a.imports_count, 1);
        assert_eq!(a.media_queries_count, 1);
        assert_eq!(a.comments_count, 1);
        assert_eq!(a.important_count, 1);
        assert_eq!(a.custom_properties_count, 1);
        assert_eq!(a.colors_count, 2);
    }

    #[test]
    fn counts_repeated_selectors_once_as_unique() {
        let a = analyze(".a {}\n.a {}\n", opts()).unwrap();
        assert_eq!(a.selectors_count, 2);
        assert_eq!(a.unique_selectors, 1);
    }

    #[test]
    fn ranks_properties_by_frequency() {
        let src = ".a { color: red; }\n.b { color: blue; margin: 0; }\n";
        let a = analyze(src, opts()).unwrap();
        assert_eq!(
            a.property_frequency,
            vec![("color".to_string(), 2), ("margin".to_string(), 1)]
        );
    }

    #[test]
    fn analyses_an_empty_sheet() {
        let a = analyze("", opts()).unwrap();
        assert_eq!(a, Analysis::default());
    }

    #[test]
    fn lists_at_rule_names() {
        let src = "@import \"a\";\n@plugin \"b\";\n@import \"c\";\n";
        let a = analyze(src, opts()).unwrap();
        assert_eq!(a.at_rule_names, vec!["import", "plugin"]);
        assert_eq!(a.at_rules_count, 3);
    }

    // -- validation ---------------------------------------------------------

    #[test]
    fn valid_css_validates() {
        let v = validate(".a { color: red; }\n", opts());
        assert!(v.valid);
        assert!(v.round_trips);
        assert_eq!(v.diagnostics, 0);
    }

    #[test]
    fn malformed_css_is_reported_but_still_round_trips() {
        let v = validate(".a { color: red;\n", opts());
        assert!(!v.valid);
        assert!(v.round_trips, "error tolerance must keep the round-trip");
        assert!(v.diagnostics > 0);
    }

    #[test]
    fn an_empty_sheet_is_valid() {
        assert!(validate("", opts()).valid);
    }

    #[test]
    fn an_error_is_placed_by_line_and_column() {
        let v = validate(".a {\n  color: red\n  background: blue;\n}\n", opts());
        assert!(!v.valid);
        let first = &v.errors[0];
        assert_eq!((first.line, first.column), (3, 13));
        assert_eq!(first.text, ": blue");
        assert!(v.message.contains("line 3, column 13"), "{}", v.message);
    }

    #[test]
    fn a_column_counts_characters_not_bytes() {
        let v = validate(".é { content: \"ü\" color: red; }", opts());
        // `: red` is the 24th character and the 26th byte.
        assert_eq!((v.errors[0].column, v.errors[0].offset), (24, 25));
    }

    // Biome 0.5.8 refuses a `:` in a value; a custom property's value may hold
    // one, and Bootstrap 3 ships `progid:` filters.
    #[test]
    fn a_colon_in_a_free_form_value_is_valid() {
        for src in [
            ".a { --x: a:b; }",
            ".a { --x: [a:b]; }",
            ".a { --x: a:b !important; }",
            ".a { filter: progid:DXImageTransform.Microsoft.gradient(startColorstr='#80000000'); }",
            ".a { -ms-filter: PROGID:DXImageTransform.Microsoft.Alpha(Opacity=80); }",
        ] {
            let v = validate(src, opts());
            assert!(v.valid, "{src}: {:?}", v.errors);
            assert!(
                v.diagnostics > 0,
                "{src} no longer trips the parser; drop the exemption"
            );
        }
    }

    #[test]
    fn a_colon_in_an_ordinary_value_is_not() {
        assert!(!validate(".a { color: red color: blue; }", opts()).valid);
        assert!(!validate(".a { --x: a:(b; }", opts()).valid);
    }

    #[test]
    fn stray_semicolons_are_nothing() {
        assert!(validate(".a { color: red;;; gap: 1px }", opts()).valid);
    }

    #[test]
    fn an_import_inside_a_block_is_invalid() {
        let v = validate(".a { @import \"x.css\"; }", opts());
        assert!(!v.valid);
        assert_eq!(v.errors[0].text, "@import \"x.css\";");
        assert!(validate("@import \"x.css\";\n.a {}\n", opts()).valid);
    }

    // -- blocks -------------------------------------------------------------

    #[test]
    fn a_block_takes_declarations_nesting_and_tailwind() {
        for body in [
            "padding: 20px;\n\n&:hover {\n  transform: translateY(-2px);\n}",
            "@media (max-width: 900px) { padding: 12px; }",
            "@supports (display: grid) { display: grid }",
            "@container card (min-width: 400px) { color: red }",
            "@layer base { color: red }",
            "@apply hover:bg-red-500 md:p-4;",
            "&:hover { @apply hover:underline md:p-2; }",
            "content: \"}\"; color: red",
            "/* } */\ngap: 1px;",
            "& .w-\\[10px\\] { gap: 1px; }",
            "body { display: none; }",
            "",
        ] {
            let v = validate_block(body, opts());
            assert!(v.valid, "{body:?}: {:?}", v.errors);
        }
    }

    #[test]
    fn a_block_error_is_placed_in_the_body() {
        let v = validate_block("gap: 1px;\ncolor: red background: blue;", opts());
        assert_eq!(v.errors.len(), 1);
        assert_eq!((v.errors[0].line, v.errors[0].column), (2, 22));
        assert_eq!(v.errors[0].offset, 31);
        assert_eq!(v.errors[0].text, ": blue");
    }

    #[test]
    fn a_block_closed_by_its_own_body_is_invalid() {
        for (body, column) in [
            ("gap: 1px; } body { display: none; ", 11),
            ("} &:hover { gap: 1px; }", 1),
            ("--x: a:b }", 10),
        ] {
            let v = validate_block(body, opts());
            assert!(!v.valid, "{body:?}");
            let early = v
                .errors
                .iter()
                .find(|e| e.message.contains("closes the block"))
                .unwrap_or_else(|| panic!("{body:?}: {:?}", v.errors));
            assert_eq!(
                (early.line, early.column, early.text.as_str()),
                (1, column, "}")
            );
        }
    }

    #[test]
    fn a_block_left_open_is_invalid_at_its_end() {
        let v = validate_block("&:hover { gap: 1px;", opts());
        assert!(!v.valid);
        let last = v.errors.last().unwrap();
        assert_eq!((last.line, last.column, last.text.as_str()), (1, 20, ""));
    }

    #[test]
    fn a_comment_or_string_left_open_is_invalid() {
        assert!(!validate_block("/* never closed\ngap: 1px;", opts()).valid);
        assert!(!validate_block("content: \"abc\ncolor: red;", opts()).valid);
        assert!(!validate_block("width: calc(1px + 2px;", opts()).valid);
        assert!(!validate_block("&[data-open { gap: 1px; }", opts()).valid);
    }

    #[test]
    fn tailwind_v4_validates() {
        let src = "@import \"tailwindcss\";\n@theme {\n  --color-x: red;\n}\n@utility tab-4 {\n  tab-size: 4;\n}\n";
        assert!(validate(src, opts()).valid);
    }
}
