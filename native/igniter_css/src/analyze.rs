// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Read-only analysis. Nothing in this module produces an edit, so none of it
//! can violate the diff-minimality constraint -- these are the queries the
//! Elixir side uses to report on a stylesheet.

use crate::ctx::{ParseCtx, ParseOptions};
use crate::error::Result;
use crate::locate::{
    all_comments, at_rule_body, declaration_lists, declarations_in_block, find_all_at_rules,
    find_all_rules, find_at_rules_named, find_top_level_rules, DeclRef,
};
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
}

/// Every **top-level** at-rule named `name`, optionally narrowed to those whose
/// target matches `matching`.
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

    query(source, options, |ctx| {
        Ok(find_at_rules_named(ctx, &wanted)
            .iter()
            .filter(|at| match matching {
                None => true,
                Some(m) => at.target.as_deref() == Some(m),
            })
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
    pub diagnostics: usize,
    pub round_trips: bool,
    pub message: String,
}

/// Is this CSS understood well enough to patch?
///
/// "Valid" here means: the parser reproduced the input byte for byte **and**
/// raised no errors. The round-trip half is the one that actually matters for
/// safety -- it is what every codemod checks before touching a file.
pub fn validate(source: &str, options: ParseOptions) -> Validation {
    if let Err(e) = crate::ctx::check_nesting(source) {
        return Validation {
            valid: false,
            diagnostics: 0,
            round_trips: false,
            message: e.to_string(),
        };
    }
    let ctx = match ParseCtx::try_new(source, options) {
        Ok(c) => c,
        Err(e) => {
            return Validation {
                valid: false,
                diagnostics: 0,
                round_trips: false,
                message: e.to_string(),
            }
        }
    };
    let round_trips = ctx.round_trips();
    let diagnostics = ctx.diagnostics_count();
    let has_errors = ctx.has_errors();
    let valid = round_trips && !has_errors;
    let message = if !round_trips {
        "the parser did not reproduce the input byte for byte; refusing to patch this file"
            .to_string()
    } else if has_errors {
        format!("CSS parsed with {diagnostics} diagnostic(s)")
    } else {
        "CSS is valid".to_string()
    };
    Validation {
        valid,
        diagnostics,
        round_trips,
        message,
    }
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
            get_at_rules(TAILWIND, "plugin", None, opts()).unwrap().len(),
            2
        );
        assert!(get_at_rules(TAILWIND, "plugin", Some("nope"), opts())
            .unwrap()
            .is_empty());
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
        assert!(found[0].text.contains("/* keeps daisyUI off our own .btn */"));
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
    fn tailwind_v4_validates() {
        let src = "@import \"tailwindcss\";\n@theme {\n  --color-x: red;\n}\n@utility tab-4 {\n  tab-size: 4;\n}\n";
        assert!(validate(src, opts()).valid);
    }
}
