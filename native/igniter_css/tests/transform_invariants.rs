// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! What must hold for the whole-file transforms against every fixture.
//!
//! `corpus_invariants` covers the mutating ops; `beautify` and `minify` reprint
//! the entire file, so they can damage a construct the ops would never touch.
//! A reprint that changes what the stylesheet *means* is the failure to catch:
//! the selector set, the declarations under each selector, and the at-rules
//! must survive, and the result must still parse.

mod support;

use igniter_css::ctx::{ParseCtx, ParseOptions};
use igniter_css::transform::{beautify, minify};
use support::fixtures;

fn opts() -> ParseOptions {
    ParseOptions::default()
}

fn parses(source: &str) -> bool {
    ParseCtx::try_new(source, opts()).is_ok_and(|ctx| ctx.round_trips())
}

/// A selector reduced to what it *means*: comments gone, whitespace collapsed,
/// and no space left around a separator. Reprinting is allowed to write `.a, .b`
/// where the source said `.a,.b`, and minifying is allowed to drop a comment —
/// neither changes which elements match.
fn canonical_selector(selector: &str) -> String {
    let chars: Vec<char> = selector.chars().collect();
    let mut stripped = String::with_capacity(selector.len());
    let mut i = 0;
    let mut in_comment = false;
    while i < chars.len() {
        if !in_comment && chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            in_comment = true;
            i += 2;
            continue;
        }
        if in_comment && chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
            in_comment = false;
            i += 2;
            continue;
        }
        if !in_comment {
            stripped.push(chars[i]);
        }
        i += 1;
    }

    let collapsed = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    let collapsed: Vec<char> = collapsed.chars().collect();
    let separator = |c: Option<char>| matches!(c, Some(',' | '>' | '+' | '~'));

    let mut out = String::with_capacity(collapsed.len());
    for (k, &c) in collapsed.iter().enumerate() {
        if c == ' ' && (separator(out.chars().last()) || separator(collapsed.get(k + 1).copied())) {
            continue;
        }
        out.push(c);
    }
    out
}

fn selectors(source: &str) -> Vec<String> {
    let mut out: Vec<String> = igniter_css::ops::rule::list_selectors(source, opts())
        .unwrap_or_default()
        .iter()
        .map(|s| canonical_selector(s))
        .collect();
    out.sort();
    out
}

/// Declarations with every space gone. Minifying is *defined* as removing
/// whitespace, so it can only be judged on what survives without it.
fn declarations_squeezed(source: &str) -> Vec<(String, String)> {
    declarations(source)
        .into_iter()
        .map(|(p, v)| (p, v.chars().filter(|c| !c.is_whitespace()).collect()))
        .collect()
}

fn declarations(source: &str) -> Vec<(String, String)> {
    let Ok(ctx) = ParseCtx::try_new(source, opts()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for rule in igniter_css::locate::find_top_level_rules(&ctx) {
        for d in igniter_css::locate::declarations_in(&ctx, &rule) {
            let value = d.value_raw.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push((d.property.clone(), value));
        }
    }
    out.sort();
    out
}

fn at_rule_names(source: &str) -> Vec<String> {
    let Ok(ctx) = ParseCtx::try_new(source, opts()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = igniter_css::locate::find_top_level_at_rules(&ctx)
        .iter()
        .map(|a| a.name.clone())
        .collect();
    out.sort();
    out
}

fn comment_count(source: &str) -> usize {
    let Ok(ctx) = ParseCtx::try_new(source, opts()) else {
        return 0;
    };
    igniter_css::locate::all_comments(&ctx).len()
}

#[test]
fn beautify_output_still_parses() {
    for (name, source) in fixtures() {
        let Ok(out) = beautify(&source, opts()) else {
            continue;
        };
        assert!(
            parses(&out),
            "{name}: beautified output does not round-trip"
        );
    }
}

#[test]
fn beautify_is_idempotent() {
    for (name, source) in fixtures() {
        let Ok(once) = beautify(&source, opts()) else {
            continue;
        };
        let twice = beautify(&once, opts()).expect("beautified output must beautify");
        assert_eq!(once, twice, "{name}: beautify is not idempotent");
    }
}

#[test]
fn beautify_keeps_every_selector() {
    for (name, source) in fixtures() {
        let Ok(out) = beautify(&source, opts()) else {
            continue;
        };
        assert_eq!(
            selectors(&source),
            selectors(&out),
            "{name}: beautify changed the selector set"
        );
    }
}

#[test]
fn beautify_keeps_every_declaration() {
    for (name, source) in fixtures() {
        let Ok(out) = beautify(&source, opts()) else {
            continue;
        };
        assert_eq!(
            declarations(&source),
            declarations(&out),
            "{name}: beautify changed a declaration"
        );
    }
}

#[test]
fn beautify_keeps_every_at_rule() {
    for (name, source) in fixtures() {
        let Ok(out) = beautify(&source, opts()) else {
            continue;
        };
        assert_eq!(
            at_rule_names(&source),
            at_rule_names(&out),
            "{name}: beautify changed the at-rules"
        );
    }
}

#[test]
fn beautify_keeps_every_comment() {
    for (name, source) in fixtures() {
        let Ok(out) = beautify(&source, opts()) else {
            continue;
        };
        assert_eq!(
            comment_count(&source),
            comment_count(&out),
            "{name}: beautify lost a comment"
        );
    }
}

#[test]
fn beautify_never_splits_a_pseudo_selector() {
    for (name, source) in fixtures() {
        let Ok(out) = beautify(&source, opts()) else {
            continue;
        };
        for selector in selectors(&out) {
            assert!(
                !selector.contains(": "),
                "{name}: beautify split a pseudo colon in {selector:?}"
            );
        }
    }
}

#[test]
fn minify_keeps_every_selector_and_declaration() {
    for (name, source) in fixtures() {
        let Ok(out) = minify(&source, opts()) else {
            continue;
        };
        assert!(parses(&out), "{name}: minified output does not round-trip");
        assert_eq!(
            selectors(&source),
            selectors(&out),
            "{name}: minify changed the selector set"
        );
        assert_eq!(
            declarations_squeezed(&source),
            declarations_squeezed(&out),
            "{name}: minify changed a declaration"
        );
    }
}

#[test]
fn beautify_then_minify_preserves_meaning() {
    for (name, source) in fixtures() {
        let Ok(pretty) = beautify(&source, opts()) else {
            continue;
        };
        let Ok(small) = minify(&pretty, opts()) else {
            continue;
        };
        assert_eq!(
            selectors(&source),
            selectors(&small),
            "{name}: round trip changed the selector set"
        );
        assert_eq!(
            declarations_squeezed(&source),
            declarations_squeezed(&small),
            "{name}: round trip changed a declaration"
        );
    }
}
