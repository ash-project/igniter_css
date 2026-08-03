// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Phase 0 GATE (ROADMAP §8).
//!
//! `parse.syntax().to_string() == source` must hold byte-for-byte across the
//! entire fixture corpus. Nothing else in this crate was allowed to exist until
//! this passed, and it stays in CI forever afterwards (§9.1).

mod support;

use igniter_css::ctx::{ParseCtx, ParseOptions};
use support::fixtures;

#[test]
fn round_trip_is_byte_identical_with_default_options() {
    for (name, source) in fixtures() {
        let ctx = ParseCtx::parse_default(&source);
        // `restore_bom` puts back the U+FEFF that `ParseCtx` strips before
        // parsing; everything between is Biome's own lossless guarantee.
        assert_eq!(
            ctx.restore_bom(ctx.syntax().to_string()),
            source,
            "lossless round-trip failed for fixture {name}"
        );
        assert!(ctx.round_trips(), "round_trips() disagrees for {name}");
    }
}

#[test]
fn round_trip_is_byte_identical_in_strict_mode() {
    for (name, source) in fixtures() {
        let ctx = ParseCtx::new(&source, ParseOptions::strict());
        assert_eq!(
            ctx.restore_bom(ctx.syntax().to_string()),
            source,
            "lossless round-trip (strict) failed for fixture {name}"
        );
    }
}

#[test]
fn round_trip_is_byte_identical_with_css_modules_enabled() {
    let opts = ParseOptions {
        allow_wrong_line_comments: true,
        css_modules: true,
    };
    for (name, source) in fixtures() {
        let ctx = ParseCtx::new(&source, opts);
        assert_eq!(
            ctx.restore_bom(ctx.syntax().to_string()),
            source,
            "lossless round-trip (css modules) failed for fixture {name}"
        );
    }
}

/// Error tolerance: fixtures that produce parse diagnostics must still
/// round-trip. This is the property the whole byte-range design rests on --
/// unparseable regions become bogus nodes that still carry their source text.
#[test]
fn round_trip_holds_even_when_the_parse_has_errors() {
    let mut saw_an_error = false;
    for (name, source) in fixtures() {
        let ctx = ParseCtx::new(&source, ParseOptions::strict());
        if ctx.has_errors() {
            saw_an_error = true;
            assert_eq!(
                ctx.restore_bom(ctx.syntax().to_string()),
                source,
                "round-trip failed for erroring fixture {name}"
            );
        }
    }
    assert!(
        saw_an_error,
        "corpus must contain at least one fixture that fails to parse cleanly, \
         otherwise this test proves nothing"
    );
}

/// R1: Tailwind v4 at-rules must not merely survive -- they must parse without
/// diagnostics, so the location layer can find them as real nodes.
#[test]
fn tailwind_v4_at_rules_parse_without_diagnostics() {
    let source = support::fixture("tailwind_v4.css");
    let ctx = ParseCtx::parse_default(&source);
    assert_eq!(
        ctx.diagnostics_count(),
        0,
        "Tailwind v4 fixture produced parse diagnostics; re-evaluate R1"
    );
    assert!(ctx.round_trips());
}
