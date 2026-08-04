// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! "Can it change any line, in any class, id or tag?"
//!
//! Table-driven coverage of every selector shape a real stylesheet uses, run
//! through the full lifecycle: update a value, append a declaration, query,
//! remove a declaration, remove the rule. The Elixir suite mirrors this table
//! through the NIF, so both sides of the boundary are covered.

use igniter_css::ctx::ParseOptions;
use igniter_css::ops::declaration::{
    get_declaration, has_declaration, remove_declaration, set_declaration, SetOptions,
};
use igniter_css::ops::rule::{has_rule, remove_rule};

fn opts() -> ParseOptions {
    ParseOptions::default()
}

/// `(label, selector, an equivalent spelling, a near miss that must NOT match)`
fn selectors() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    vec![
        ("class", ".btn", ".btn", ".btn-primary"),
        ("id", "#main", "#main", "#mai"),
        ("tag", "div", "div", "div span"),
        ("universal", "*", "*", "*.x"),
        (
            "attribute",
            r#"a[href^="https://"]"#,
            r#"a[href^="https://"]"#,
            r#"a[href^="http://"]"#,
        ),
        (
            "attribute bare",
            "[data-phx-session]",
            "[data-phx-session]",
            "[data-phx]",
        ),
        ("pseudo class", "a:hover", "a:hover", "a:focus"),
        (
            "pseudo element",
            "p::first-line",
            "p::first-line",
            "p::first-letter",
        ),
        (
            "functional pseudo",
            "li:nth-child(2n+1)",
            "li:nth-child(2n+1)",
            "li:nth-child(2n)",
        ),
        (
            "not()",
            "input:not([disabled])",
            "input:not([disabled])",
            "input:not([readonly])",
        ),
        (
            "where()",
            ":where(h1, h2)",
            ":where(h1, h2)",
            ":where(h1, h3)",
        ),
        ("root", ":root", ":root", ":host"),
        ("descendant", "nav ul li", "nav    ul   li", "nav ul"),
        ("child", ".a > .b", ".a>.b", ".a .b"),
        ("adjacent sibling", ".a + .b", ".a+.b", ".a ~ .b"),
        ("general sibling", ".a ~ .b", ".a~.b", ".a + .b"),
        ("selector list", ".a, .b", ".a,.b", ".a"),
        (
            "tag with class",
            "button.primary",
            "button.primary",
            "button",
        ),
        (
            "compound chain",
            "#app .card > h2:first-child",
            "#app .card>h2:first-child",
            "#app .card h2",
        ),
        ("escaped slash", r".w-1\/2", r".w-1\/2", ".w-1"),
        ("non ascii", ".café", ".café", ".cafe"),
        ("double class", ".a.b", ".a.b", ".a .b"),
    ]
}

/// A rule carrying a comment in every awkward position, so each lifecycle step
/// also proves comment survival for that selector shape.
fn rule_for(selector: &str) -> String {
    format!(
        "/* above {selector} */\n{selector} {{\n  color: red; /* trailing */\n  margin: 0;\n}}\n"
    )
}

#[test]
fn every_selector_shape_supports_the_full_lifecycle() {
    for (label, selector, _, _) in selectors() {
        let src = rule_for(selector);

        // 1. update an existing value -- comment on that line must survive
        let updated = set_declaration(
            &src,
            selector,
            "color",
            "blue",
            SetOptions::default(),
            opts(),
        )
        .unwrap_or_else(|e| panic!("{label}: set failed: {e}"));
        assert!(updated.changed, "{label}: set reported no change");
        assert!(
            updated.source.contains("color: blue; /* trailing */"),
            "{label}: value-only replacement failed\n{}",
            updated.source
        );
        assert!(
            updated.source.contains(&format!("/* above {selector} */")),
            "{label}: lost the leading comment"
        );

        // 2. append a new declaration
        let appended = set_declaration(
            &updated.source,
            selector,
            "padding",
            "1rem",
            SetOptions::default(),
            opts(),
        )
        .unwrap_or_else(|e| panic!("{label}: append failed: {e}"));
        assert!(
            appended.source.contains("padding: 1rem;"),
            "{label}: append missing"
        );

        // 3. queries
        assert!(
            has_rule(&appended.source, selector, opts()).unwrap(),
            "{label}: has_rule false"
        );
        assert!(
            has_declaration(&appended.source, selector, "padding", opts()).unwrap(),
            "{label}: has_declaration false"
        );
        assert_eq!(
            get_declaration(&appended.source, selector, "color", opts()).unwrap(),
            Some("blue".to_string()),
            "{label}: get_declaration wrong"
        );

        // 4. remove a declaration
        let removed = remove_declaration(&appended.source, selector, "padding", opts())
            .unwrap_or_else(|e| panic!("{label}: remove_declaration failed: {e}"));
        assert!(
            !removed.source.contains("padding: 1rem;"),
            "{label}: not removed"
        );

        // 5. remove the rule entirely
        let gone = remove_rule(&removed.source, selector, opts())
            .unwrap_or_else(|e| panic!("{label}: remove_rule failed: {e}"));
        assert!(gone.changed, "{label}: remove_rule reported no change");
        assert!(
            !has_rule(&gone.source, selector, opts()).unwrap(),
            "{label}: rule survived removal"
        );
    }
}

#[test]
fn every_selector_shape_is_idempotent() {
    for (label, selector, _, _) in selectors() {
        let src = rule_for(selector);

        let once = set_declaration(
            &src,
            selector,
            "padding",
            "1rem",
            SetOptions::default(),
            opts(),
        )
        .unwrap();
        let twice = set_declaration(
            &once.source,
            selector,
            "padding",
            "1rem",
            SetOptions::default(),
            opts(),
        )
        .unwrap();
        assert_eq!(once.source, twice.source, "{label}: set not idempotent");
        assert!(!twice.changed, "{label}: set changed on second run");

        let once = remove_rule(&src, selector, opts()).unwrap();
        let twice = remove_rule(&once.source, selector, opts()).unwrap();
        assert_eq!(once.source, twice.source, "{label}: remove not idempotent");
        assert!(!twice.changed, "{label}: remove changed on second run");
    }
}

#[test]
fn an_equivalent_spelling_matches_and_a_near_miss_does_not() {
    for (label, selector, equivalent, near_miss) in selectors() {
        let src = rule_for(selector);

        assert!(
            has_rule(&src, equivalent, opts()).unwrap(),
            "{label}: {equivalent:?} should match {selector:?}"
        );
        assert!(
            !has_rule(&src, near_miss, opts()).unwrap(),
            "{label}: {near_miss:?} must NOT match {selector:?}"
        );

        // And the equivalent spelling can drive a real edit.
        let out = set_declaration(
            &src,
            equivalent,
            "color",
            "green",
            SetOptions::default(),
            opts(),
        )
        .unwrap_or_else(|e| panic!("{label}: edit via {equivalent:?} failed: {e}"));
        assert!(out.changed, "{label}: edit via {equivalent:?} did nothing");
    }
}

#[test]
fn every_selector_shape_survives_a_round_trip_unchanged_when_nothing_applies() {
    for (label, selector, _, near_miss) in selectors() {
        let src = rule_for(selector);

        // Removing a rule that isn't there must not touch the file.
        let out = remove_rule(&src, near_miss, opts()).unwrap();
        assert!(
            !out.changed,
            "{label}: near miss {near_miss:?} removed something"
        );
        assert_eq!(out.source, src, "{label}: file changed for a no-op");
    }
}

/// Declarations are edited by property, so every property shape must work too.
#[test]
fn every_property_shape_can_be_set_and_removed() {
    let properties = [
        ("standard", "color", "red"),
        ("hyphenated", "background-color", "#fff"),
        ("custom property", "--brand", "#4f46e5"),
        ("vendor prefixed", "-webkit-user-select", "none"),
        ("shorthand", "margin", "0 auto 10px"),
        ("function value", "background", "var(--x, #fff)"),
        ("multi function", "transform", "translate(1px) rotate(2deg)"),
        (
            "url value",
            "background-image",
            "url(data:image/svg+xml;base64,AA==)",
        ),
        ("string value", "content", "\"→ ✨\""),
        (
            "grid template",
            "grid-template-columns",
            "minmax(12rem, 1fr) 3fr",
        ),
    ];

    for (label, property, value) in properties {
        let src = ".x {\n  z-index: 1;\n}\n";
        let out = set_declaration(src, ".x", property, value, SetOptions::default(), opts())
            .unwrap_or_else(|e| panic!("{label}: set failed: {e}"));
        assert!(out.changed, "{label}: nothing changed");
        assert_eq!(
            get_declaration(&out.source, ".x", property, opts()).unwrap(),
            Some(value.to_string()),
            "{label}: value did not round-trip"
        );

        let gone = remove_declaration(&out.source, ".x", property, opts()).unwrap();
        assert_eq!(
            gone.source, src,
            "{label}: removal did not restore the original"
        );
    }
}
