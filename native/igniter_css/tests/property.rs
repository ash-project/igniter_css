// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Property tests over generated CSS.
//!
//! Unit tests check the cases we thought of. These check the invariants against
//! input nobody wrote by hand -- including input that is not valid CSS at all,
//! since a codemod must never panic on a user's file however broken it is.

use igniter_css::ctx::{ParseCtx, ParseOptions};
use igniter_css::edit::{apply_edits, Edit};
use igniter_css::ops::at_rule::ensure_at_rule_line;
use igniter_css::ops::declaration::{set_declaration, SetOptions};
use igniter_css::ops::rule::{ensure_rule, remove_rule};
use igniter_css::ops::tidy::{remove_duplicates, sort_properties, DedupeOptions};
use proptest::prelude::*;

fn opts() -> ParseOptions {
    ParseOptions::default()
}

// ---------------------------------------------------------------------------
// Generators
// ---------------------------------------------------------------------------

fn ident() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "a",
        "b",
        "header",
        "footer",
        "btn",
        "card",
        "x-1",
        "hide-scrollbar",
    ])
    .prop_map(String::from)
}

fn property() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "color",
        "margin",
        "padding",
        "display",
        "--brand",
        "user-select",
        "z-index",
    ])
    .prop_map(String::from)
}

fn value() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "red",
        "0",
        "1px solid #333",
        "none",
        "var(--brand)",
        "0 auto 10px",
        "\"quoted ; value\"",
    ])
    .prop_map(String::from)
}

fn declaration() -> impl Strategy<Value = String> {
    (property(), value(), any::<bool>(), 0u8..3).prop_map(|(p, v, important, comment)| {
        let imp = if important { " !important" } else { "" };
        match comment {
            1 => format!("  /* about {p} */\n  {p}: {v}{imp};"),
            2 => format!("  {p}: {v}{imp}; /* trailing */"),
            _ => format!("  {p}: {v}{imp};"),
        }
    })
}

fn rule() -> impl Strategy<Value = String> {
    (ident(), prop::collection::vec(declaration(), 0..4), 0u8..3).prop_map(
        |(name, decls, shape)| {
            let selector = match shape {
                1 => format!(".{name} > .inner"),
                2 => format!("#{name}, .{name}"),
                _ => format!(".{name}"),
            };
            format!("{selector} {{\n{}\n}}", decls.join("\n"))
        },
    )
}

fn at_rule() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "@import \"tailwindcss\";",
        "@plugin \"../vendor/daisyui\";",
        "@source \"../js\";",
        "@theme {\n  --color-x: red;\n}",
        "@media print {\n  .p {\n    display: none;\n  }\n}",
        "@layer base, components;",
    ])
    .prop_map(String::from)
}

fn stylesheet() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            rule(),
            at_rule(),
            Just("/* a comment */".to_string()),
            Just("/* ===== Section ===== */".to_string()),
        ],
        0..7,
    )
    .prop_map(|parts| {
        let body = parts.join("\n\n");
        if body.is_empty() {
            body
        } else {
            format!("{body}\n")
        }
    })
}

/// Arbitrary bytes that are *probably* not valid CSS.
fn junk() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "{", "}", ";", ":", "/*", "*/", "//", "\"", "'", "@", ".x", "url(", ")", "\n", "  ",
            "\r\n", "é", "\u{feff}",
        ]),
        0..24,
    )
    .prop_map(|v| v.concat())
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

proptest! {
    /// The round-trip gate, generalised: the parse must reproduce any input.
    #[test]
    fn round_trip_holds_for_generated_stylesheets(src in stylesheet()) {
        let ctx = ParseCtx::parse_default(&src);
        prop_assert_eq!(ctx.restore_bom(ctx.syntax().to_string()), src);
    }

    #[test]
    fn round_trip_holds_for_junk(src in junk()) {
        let ctx = ParseCtx::parse_default(&src);
        prop_assert_eq!(ctx.restore_bom(ctx.syntax().to_string()), src);
    }

    /// Non-overlapping edits splice cleanly and the result still parses.
    #[test]
    fn applying_non_overlapping_edits_yields_a_parseable_file(
        src in stylesheet(),
        cuts in prop::collection::vec(0usize..400, 0..6),
    ) {
        // Snap each offset to a char boundary, sort, and pair them up so no two
        // ranges overlap.
        let mut offsets: Vec<usize> = cuts
            .into_iter()
            .map(|c| {
                let mut c = c.min(src.len());
                while c > 0 && !src.is_char_boundary(c) {
                    c -= 1;
                }
                c
            })
            .collect();
        offsets.sort_unstable();
        offsets.dedup();

        let edits: Vec<Edit> = offsets
            .chunks(2)
            .filter(|w| w.len() == 2 && w[0] < w[1])
            .map(|w| Edit::replace(w[0], w[1], "/*x*/"))
            .collect();

        let out = apply_edits(&src, edits).expect("non-overlapping edits must apply");
        let ctx = ParseCtx::parse_default(&out);
        prop_assert!(ctx.round_trips());
    }

    /// Overlapping edits are always rejected, never silently merged.
    #[test]
    fn overlapping_edits_are_always_rejected(src in stylesheet(), a in 0usize..50, len in 1usize..20) {
        prop_assume!(src.len() > 80);
        let mut a = a.min(src.len());
        while a > 0 && !src.is_char_boundary(a) { a -= 1; }
        let mut b = (a + len).min(src.len());
        while b > a && !src.is_char_boundary(b) { b -= 1; }
        let mut c = (a + len / 2).min(src.len());
        while c > a && !src.is_char_boundary(c) { c -= 1; }
        let mut d = (b + len).min(src.len());
        while d > c && !src.is_char_boundary(d) { d -= 1; }
        prop_assume!(a < c && c < b && b < d);

        let result = apply_edits(&src, vec![Edit::replace(a, b, "X"), Edit::replace(c, d, "Y")]);
        prop_assert!(result.is_err());
    }

    /// No op may panic, whatever it is handed.
    #[test]
    fn ops_never_panic_on_junk(src in junk()) {
        let _ = ensure_at_rule_line(&src, "@plugin \"p\";", opts());
        let _ = ensure_rule(&src, ".probe", opts());
        let _ = remove_rule(&src, ".probe", opts());
        let _ = set_declaration(&src, ".probe", "color", "red", SetOptions { create_rule: true, ..Default::default() }, opts());
        let _ = sort_properties(&src, opts());
        let _ = remove_duplicates(&src, DedupeOptions::default(), opts());
        let _ = igniter_css::transform::minify(&src, opts());
        let _ = igniter_css::transform::beautify(&src, opts());
        let _ = igniter_css::analyze::analyze(&src, opts());
        let _ = igniter_css::analyze::extract_colors(&src, opts());
        let _ = igniter_css::analyze::extract_animations(&src, opts());
        let _ = igniter_css::analyze::validate(&src, opts());
    }

    /// Idempotency, over generated input rather than a fixed corpus.
    #[test]
    fn ensure_at_rule_is_idempotent(src in stylesheet()) {
        let Ok(once) = ensure_at_rule_line(&src, "@plugin \"probe\";", opts()) else { return Ok(()) };
        let twice = ensure_at_rule_line(&once.source, "@plugin \"probe\";", opts()).unwrap();
        prop_assert_eq!(&once.source, &twice.source);
        prop_assert!(!twice.changed);
    }

    #[test]
    fn ensure_rule_is_idempotent(src in stylesheet()) {
        let Ok(once) = ensure_rule(&src, ".igniter-probe", opts()) else { return Ok(()) };
        let twice = ensure_rule(&once.source, ".igniter-probe", opts()).unwrap();
        prop_assert_eq!(&once.source, &twice.source);
        prop_assert!(!twice.changed);
    }

    #[test]
    fn sorting_is_idempotent(src in stylesheet()) {
        let Ok(once) = sort_properties(&src, opts()) else { return Ok(()) };
        let twice = sort_properties(&once.source, opts()).unwrap();
        prop_assert_eq!(&once.source, &twice.source);
    }

    /// Insertion ops never lose a comment.
    #[test]
    fn insertion_ops_preserve_every_comment(src in stylesheet()) {
        let count_comments = |s: &str| {
            let ctx = ParseCtx::parse_default(s);
            igniter_css::locate::all_comments(&ctx).len()
        };
        let before = count_comments(&src);
        if let Ok(o) = ensure_at_rule_line(&src, "@plugin \"probe\";", opts()) {
            prop_assert_eq!(count_comments(&o.source), before);
        }
        if let Ok(o) = ensure_rule(&src, ".igniter-probe", opts()) {
            prop_assert_eq!(count_comments(&o.source), before);
        }
    }

    /// Minified output is never larger and always still parses.
    #[test]
    fn minifying_shrinks_and_stays_valid(src in stylesheet()) {
        let out = igniter_css::transform::minify(&src, opts()).unwrap();
        prop_assert!(out.len() <= src.len());
        let ctx = ParseCtx::parse_default(&out);
        prop_assert!(ctx.round_trips());
    }

    /// Beautify then minify lands on the same text as minify alone.
    #[test]
    fn beautify_does_not_change_what_a_sheet_means(src in stylesheet()) {
        let direct = igniter_css::transform::minify(&src, opts()).unwrap();
        let pretty = igniter_css::transform::beautify(&src, opts()).unwrap();
        let via_pretty = igniter_css::transform::minify(&pretty, opts()).unwrap();
        prop_assert_eq!(direct, via_pretty);
    }
}
