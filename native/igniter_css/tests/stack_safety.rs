// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Deeply nested input must be refused, not parsed.
//!
//! Biome's CSS parser is recursive descent, so nesting past a few thousand
//! levels overflows the stack. That is an abort, not a panic: `catch_unwind`
//! cannot intercept it, so inside a NIF it would take the VM down instead of
//! raising in the calling process. Every entry point that reaches a parse has
//! to reject such input first.
//!
//! These cases are all well below the depth that actually aborts -- the point
//! is that the guard fires, not that we survive an overflow.

use igniter_css::analyze;
use igniter_css::ctx::{check_nesting, nesting_depth, ParseOptions, MAX_NESTING_DEPTH};
use igniter_css::ops::at_rule::ensure_at_rule_line;
use igniter_css::ops::declaration::{set_declaration, SetOptions};
use igniter_css::ops::rule::{ensure_rule, has_rule};
use igniter_css::ops::tidy::{remove_duplicates, sort_properties, DedupeOptions};
use igniter_css::transform;

fn opts() -> ParseOptions {
    ParseOptions::default()
}

fn nested_blocks(depth: usize) -> String {
    format!(
        "{}.a{{color:red}}{}",
        "@media print{".repeat(depth),
        "}".repeat(depth)
    )
}

fn nested_selector(depth: usize) -> String {
    format!("{}a{}", ":not(".repeat(depth), ")".repeat(depth))
}

#[test]
fn depth_is_measured_without_recursing() {
    assert_eq!(nesting_depth(""), 0);
    assert_eq!(nesting_depth(".a { color: red; }"), 1);
    assert_eq!(nesting_depth("@media print { .a { color: red; } }"), 2);
    assert_eq!(nesting_depth(".a { background: url(x) }"), 2);
    assert_eq!(nesting_depth(&nested_blocks(50)), 51);
    // Braces inside strings and comments are not nesting.
    assert_eq!(nesting_depth(r#".a { content: "{{{{" }"#), 1);
    assert_eq!(nesting_depth(".a { /* {{{{ */ color: red }"), 1);
}

#[test]
fn ordinary_css_is_nowhere_near_the_limit() {
    for source in [
        ".a { color: red; }",
        "@media print { @supports (display: grid) { .a { color: red; } } }",
        ".a { background: url(data:image/svg+xml;base64,AA==); }",
    ] {
        assert!(
            check_nesting(source).is_ok(),
            "rejected ordinary CSS: {source}"
        );
        assert!(nesting_depth(source) < 10);
    }
}

#[test]
fn every_mutating_op_refuses_deeply_nested_input() {
    let src = nested_blocks(MAX_NESTING_DEPTH + 1);

    assert!(ensure_at_rule_line(&src, "@plugin \"p\";", opts()).is_err());
    assert!(ensure_rule(&src, ".probe", opts()).is_err());
    assert!(sort_properties(&src, opts()).is_err());
    assert!(remove_duplicates(&src, DedupeOptions::default(), opts()).is_err());
    assert!(set_declaration(
        &src,
        ".a",
        "color",
        "blue",
        SetOptions {
            create_rule: true,
            ..Default::default()
        },
        opts()
    )
    .is_err());
}

#[test]
fn every_read_only_op_refuses_deeply_nested_input() {
    let src = nested_blocks(MAX_NESTING_DEPTH + 1);

    assert!(analyze::analyze(&src, opts()).is_err());
    assert!(analyze::extract_colors(&src, opts()).is_err());
    assert!(analyze::extract_media_queries(&src, opts()).is_err());
    assert!(analyze::extract_animations(&src, opts()).is_err());
    assert!(has_rule(&src, ".a", opts()).is_err());

    // `validate` reports rather than returning Result, so it must say so.
    let v = analyze::validate(&src, opts());
    assert!(!v.valid);
    assert!(v.message.contains("nested"));
}

#[test]
fn transforms_refuse_deeply_nested_input() {
    let src = nested_blocks(MAX_NESTING_DEPTH + 1);
    assert!(transform::minify(&src, opts()).is_err());
    assert!(transform::beautify(&src, opts()).is_err());
    assert!(transform::merge_stylesheets(&[src], opts()).is_err());
}

#[test]
fn a_deeply_nested_selector_argument_is_refused_too() {
    // The caller's selector is parsed as well, so it needs the same guard.
    let deep = nested_selector(MAX_NESTING_DEPTH + 1);
    assert!(has_rule(".a { color: red; }", &deep, opts()).is_ok());
    assert!(ensure_rule(".a {}", &deep, opts()).is_err());
    assert!(ensure_at_rule_line("", &format!("@media {deep} {{ }}"), opts()).is_err());
}

#[test]
fn the_error_says_what_happened() {
    let src = nested_blocks(MAX_NESTING_DEPTH + 1);
    let err = ensure_rule(&src, ".probe", opts()).unwrap_err().to_string();
    assert!(err.contains("nested"), "unhelpful error: {err}");
    assert!(
        err.contains(&MAX_NESTING_DEPTH.to_string()),
        "no limit given: {err}"
    );
}

/// Biome 0.5.8 panics on some inputs with "The parser is no longer
/// progressing". This one was found by the property tests and shrunk by
/// proptest; the seed is kept in tests/property.proptest-regressions.
///
/// Rustler would turn the panic into an exception in the calling process, which
/// the VM survives but which breaks the promise that unpatchable input yields
/// `{:error, reason}`. Every entry point must return an error instead.
mod parser_panics {
    use super::*;

    /// `@layer` followed immediately by a comment, then an orphaned
    /// declaration and a stray brace.
    const PANICS_BIOME: &str = "@layer/*x*/\n  color: var(--brand);\n}\n";

    #[test]
    fn the_input_still_panics_the_underlying_parser() {
        // If this ever stops panicking, Biome has fixed it upstream and the
        // guard below is belt and braces rather than load-bearing.
        let panicked = std::panic::catch_unwind(|| {
            let _ = igniter_css::ctx::ParseCtx::parse_default(PANICS_BIOME);
        })
        .is_err();
        assert!(
            panicked,
            "biome no longer panics on this input -- the guard may be removable"
        );
    }

    #[test]
    fn every_entry_point_returns_an_error_rather_than_unwinding() {
        assert!(ensure_at_rule_line(PANICS_BIOME, "@plugin \"p\";", opts()).is_err());
        assert!(ensure_rule(PANICS_BIOME, ".probe", opts()).is_err());
        assert!(sort_properties(PANICS_BIOME, opts()).is_err());
        assert!(remove_duplicates(PANICS_BIOME, DedupeOptions::default(), opts()).is_err());
        assert!(has_rule(PANICS_BIOME, ".a", opts()).is_err());
        assert!(analyze::analyze(PANICS_BIOME, opts()).is_err());
        assert!(analyze::extract_colors(PANICS_BIOME, opts()).is_err());
        assert!(transform::minify(PANICS_BIOME, opts()).is_err());
        assert!(transform::beautify(PANICS_BIOME, opts()).is_err());

        let v = analyze::validate(PANICS_BIOME, opts());
        assert!(!v.valid);
        assert!(!v.round_trips);
    }

    #[test]
    fn the_error_is_the_documented_one() {
        let err = ensure_rule(PANICS_BIOME, ".probe", opts()).unwrap_err();
        assert!(
            matches!(err, igniter_css::error::CssError::Unparseable(_)),
            "expected Unparseable, got {err:?}"
        );
    }
}
