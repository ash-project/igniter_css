// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! The invariants that must hold for **every op** against **every
//! fixture**, not just for the cases somebody remembered to write a unit test
//! for.
//!
//! 1. idempotency -- applying twice equals applying once, and the second run
//!    reports `changed: false` (§9.3)
//! 2. comment preservation -- no comment is ever lost (§2 constraint 1)
//! 3. diff minimality -- the changed-line count stays within budget (§9.4)
//! 4. output validity -- the result still round-trips and gains no new parse
//!    errors
//! 5. malformed input -- an error is returned and the source is untouched (§9.5)

mod support;

use igniter_css::analyze;
use igniter_css::ctx::{ParseCtx, ParseOptions};
use igniter_css::ops::at_rule::{ensure_at_rule_line, remove_at_rule};
use igniter_css::ops::declaration::{
    add_vendor_prefixes, remove_declaration, set_declaration, SetOptions,
};
use igniter_css::ops::rule::{append_raw_to_rule, ensure_rule, remove_rule, replace_rule_body};
use igniter_css::ops::tidy::{remove_duplicates, sort_properties, DedupeOptions};
use igniter_css::ops::Outcome;
use support::{changed_line_count, fixtures};

type Op = (&'static str, fn(&str) -> Option<Outcome>);

fn opts() -> ParseOptions {
    ParseOptions::default()
}

/// Every mutating op, wrapped so an expected error (an ambiguous selector, a
/// missing rule) becomes `None` rather than a panic. `None` means "this op does
/// not apply to this fixture", which is itself valid behaviour.
fn ops() -> Vec<Op> {
    vec![
        ("ensure_at_rule_import", |s| {
            ensure_at_rule_line(s, "@import \"igniter-probe.css\";", opts()).ok()
        }),
        ("ensure_at_rule_plugin", |s| {
            ensure_at_rule_line(s, "@plugin \"igniter-probe\";", opts()).ok()
        }),
        ("ensure_at_rule_source", |s| {
            ensure_at_rule_line(s, "@source \"../igniter-probe\";", opts()).ok()
        }),
        ("remove_at_rule_import", |s| {
            remove_at_rule(s, "import", None, opts()).ok()
        }),
        ("remove_at_rule_plugin", |s| {
            remove_at_rule(s, "plugin", None, opts()).ok()
        }),
        ("ensure_rule", |s| {
            ensure_rule(s, ".igniter-probe", opts()).ok()
        }),
        ("remove_rule", |s| {
            remove_rule(s, ".hide-scrollbar", opts()).ok()
        }),
        ("set_declaration_new_rule", |s| {
            set_declaration(
                s,
                ".igniter-probe",
                "display",
                "none",
                SetOptions {
                    create_rule: true,
                    ..Default::default()
                },
                opts(),
            )
            .ok()
        }),
        ("set_declaration_existing", |s| {
            set_declaration(
                s,
                ".page",
                "color",
                "rebeccapurple",
                SetOptions::default(),
                opts(),
            )
            .ok()
        }),
        // `.page { display: flex; /* trailing on a declaration */ }` in the
        // comments fixture: this is the only op that takes the *update* branch
        // on a declaration that already carries a trailing comment, so without
        // it the sweep never exercises rule E against a real comment.
        ("set_declaration_over_a_commented_line", |s| {
            set_declaration(
                s,
                ".page",
                "display",
                "block",
                SetOptions::default(),
                opts(),
            )
            .ok()
        }),
        ("remove_declaration", |s| {
            remove_declaration(s, ".page", "display", opts()).ok()
        }),
        ("append_raw_to_rule", |s| {
            append_raw_to_rule(s, ".page", "outline: 1px solid red;", opts()).ok()
        }),
        ("replace_rule_body", |s| {
            replace_rule_body(s, ".sr-only", "position: fixed;", opts()).ok()
        }),
        ("add_vendor_prefixes", |s| {
            add_vendor_prefixes(
                s,
                "user-select",
                &["-webkit-".to_string(), "-moz-".to_string()],
                opts(),
            )
            .ok()
        }),
        ("add_vendor_prefixes_display", |s| {
            add_vendor_prefixes(s, "display", &["-webkit-".to_string()], opts()).ok()
        }),
        ("sort_properties", |s| sort_properties(s, opts()).ok()),
        ("remove_duplicates", |s| {
            remove_duplicates(s, DedupeOptions::default(), opts()).ok()
        }),
    ]
}

/// Comment texts present in a source, as a multiset.
fn comment_texts(source: &str) -> Vec<String> {
    let ctx = ParseCtx::parse_default(source);
    let mut v: Vec<String> = igniter_css::locate::all_comments(&ctx)
        .into_iter()
        .map(|(_, _, t)| t)
        .collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------

#[test]
fn every_op_is_idempotent_on_every_fixture() {
    for (name, source) in fixtures() {
        for (op_name, op) in ops() {
            let Some(once) = op(&source) else { continue };
            let Some(twice) = op(&once.source) else {
                panic!("{op_name} succeeded on {name} but failed on its own output");
            };
            assert_eq!(
                once.source, twice.source,
                "{op_name} is not idempotent on {name}"
            );
            assert!(
                !twice.changed,
                "{op_name} reported changed=true on the second run over {name}"
            );
        }
    }
}

#[test]
fn no_op_ever_loses_a_comment() {
    for (name, source) in fixtures() {
        let before = comment_texts(&source);
        if before.is_empty() {
            continue;
        }
        for (op_name, op) in ops() {
            // Removal ops delete comments on purpose (rule D); they are covered
            // by their own targeted tests.
            if op_name.starts_with("remove_") {
                continue;
            }
            let Some(out) = op(&source) else { continue };
            let after = comment_texts(&out.source);
            for c in &before {
                assert!(
                    after.contains(c),
                    "{op_name} lost comment {c:?} from {name}"
                );
            }
        }
    }
}

#[test]
fn every_op_leaves_the_output_parseable_and_lossless() {
    for (name, source) in fixtures() {
        let before = ParseCtx::parse_default(&source);
        let before_errors = before.diagnostics_count();

        for (op_name, op) in ops() {
            let Some(out) = op(&source) else { continue };
            let after = ParseCtx::parse_default(&out.source);
            assert!(
                after.round_trips(),
                "{op_name} produced output that does not round-trip, from {name}"
            );
            assert!(
                after.diagnostics_count() <= before_errors,
                "{op_name} introduced {} new parse diagnostic(s) into {name}",
                after.diagnostics_count() - before_errors
            );
        }
    }
}

#[test]
fn every_op_preserves_the_files_newline_style() {
    for (name, source) in fixtures() {
        if !source.contains("\r\n") {
            continue;
        }
        let lf_only_before = source.matches('\n').count() - source.matches("\r\n").count();
        for (op_name, op) in ops() {
            let Some(out) = op(&source) else { continue };
            let lf_only_after =
                out.source.matches('\n').count() - out.source.matches("\r\n").count();
            assert_eq!(
                lf_only_before, lf_only_after,
                "{op_name} introduced a bare LF into the CRLF file {name}"
            );
        }
    }
}

#[test]
fn every_op_preserves_a_bom() {
    for (name, source) in fixtures() {
        let had_bom = source.starts_with('\u{feff}');
        for (op_name, op) in ops() {
            let Some(out) = op(&source) else { continue };
            assert_eq!(
                out.source.starts_with('\u{feff}'),
                had_bom,
                "{op_name} changed the BOM state of {name}"
            );
        }
    }
}

#[test]
fn an_unchanged_outcome_returns_the_source_byte_for_byte() {
    for (name, source) in fixtures() {
        for (op_name, op) in ops() {
            let Some(out) = op(&source) else { continue };
            if !out.changed {
                assert_eq!(
                    out.source, source,
                    "{op_name} reported changed=false but altered {name}"
                );
            }
        }
    }
}

/// §9.4: a codemod touching one thing must not reformat the file around it.
#[test]
fn single_target_ops_change_only_a_handful_of_lines() {
    // (op, budget in changed lines). Generous, but far below "whole file".
    type BudgetedOp = (&'static str, fn(&str) -> Option<Outcome>, usize);
    let cases: Vec<BudgetedOp> = vec![
        (
            "ensure_at_rule_line",
            |s| ensure_at_rule_line(s, "@plugin \"igniter-probe\";", opts()).ok(),
            1,
        ),
        (
            "ensure_rule",
            |s| ensure_rule(s, ".igniter-probe", opts()).ok(),
            4,
        ),
        (
            "set_declaration",
            |s| {
                set_declaration(
                    s,
                    ".page",
                    "color",
                    "rebeccapurple",
                    SetOptions::default(),
                    opts(),
                )
                .ok()
            },
            2,
        ),
        (
            // A declaration plus the comments Rule D says it owns.
            "remove_declaration",
            |s| remove_declaration(s, ".page", "display", opts()).ok(),
            3,
        ),
    ];

    for (name, source) in fixtures() {
        for (op_name, op, budget) in &cases {
            let Some(out) = op(&source) else { continue };
            if !out.changed {
                continue;
            }
            let changed = changed_line_count(&source, &out.source);
            assert!(
                changed <= *budget,
                "{op_name} changed {changed} lines in {name} (budget {budget})"
            );
        }
    }
}

/// §9.5: input we cannot understand well enough to patch must come back
/// untouched, with an error, never half-edited.
#[test]
fn malformed_input_is_never_half_edited() {
    let malformed = [
        ".broken {\n  color: red;\n",
        ".a { color: red; }\n}\n.b { color: blue; }\n",
        "{{{{",
        "@media (\n",
        "}",
        ".a { color: ",
    ];

    for source in malformed {
        for (op_name, op) in ops() {
            // An op that fails cannot have written anything -- guaranteed by the
            // API shape, since an error carries no source. One that succeeds
            // must still have produced valid output.
            if let Some(out) = op(source) {
                let ctx = ParseCtx::parse_default(&out.source);
                assert!(
                    ctx.round_trips(),
                    "{op_name} produced non-round-tripping output from {source:?}"
                );
                if !out.changed {
                    assert_eq!(out.source, source);
                }
            }
        }
    }
}

/// The read-only ops must never panic, whatever we hand them.
#[test]
fn analysis_ops_survive_the_whole_corpus() {
    for (name, source) in fixtures() {
        analyze::analyze(&source, opts()).unwrap_or_else(|e| panic!("analyze {name}: {e}"));
        analyze::extract_colors(&source, opts())
            .unwrap_or_else(|e| panic!("extract_colors {name}: {e}"));
        analyze::extract_media_queries(&source, opts())
            .unwrap_or_else(|e| panic!("extract_media_queries {name}: {e}"));
        analyze::extract_animations(&source, opts())
            .unwrap_or_else(|e| panic!("extract_animations {name}: {e}"));
        let _ = analyze::validate(&source, opts());
    }
}

#[test]
fn transforms_survive_the_whole_corpus_and_stay_parseable() {
    for (name, source) in fixtures() {
        let minified = igniter_css::transform::minify(&source, opts())
            .unwrap_or_else(|e| panic!("minify {name}: {e}"));
        let ctx = ParseCtx::parse_default(&minified);
        assert!(ctx.round_trips(), "minified {name} does not round-trip");

        let pretty = igniter_css::transform::beautify(&source, opts())
            .unwrap_or_else(|e| panic!("beautify {name}: {e}"));
        let ctx = ParseCtx::parse_default(&pretty);
        assert!(ctx.round_trips(), "beautified {name} does not round-trip");

        // Beautifying keeps every comment; minifying deliberately does not.
        let before = comment_texts(&source);
        let after = comment_texts(&pretty);
        for c in &before {
            assert!(after.contains(c), "beautify lost comment {c:?} from {name}");
        }
    }
}

#[test]
fn minifying_never_grows_a_file() {
    for (name, source) in fixtures() {
        let minified = igniter_css::transform::minify(&source, opts()).unwrap();
        assert!(
            minified.len() <= source.len(),
            "minifying grew {name} from {} to {} bytes",
            source.len(),
            minified.len()
        );
    }
}

/// The end-to-end shape a real Igniter installer takes: patch a fresh Phoenix
/// `app.css` and check the diff contains only the lines we meant to add.
#[test]
fn a_phoenix_app_css_can_be_patched_with_a_minimal_diff() {
    let source = support::fixture("phoenix_app.css");

    let step1 =
        ensure_at_rule_line(&source, "@plugin \"@tailwindcss/typography\";", opts()).unwrap();
    let step2 = ensure_at_rule_line(&step1.source, "@source \"../vendor\";", opts()).unwrap();
    let step3 = ensure_rule(&step2.source, ".hide-scrollbar", opts()).unwrap();
    let step4 = set_declaration(
        &step3.source,
        ".hide-scrollbar",
        "scrollbar-width",
        "none",
        SetOptions::default(),
        opts(),
    )
    .unwrap();

    assert!(step1.changed && step2.changed && step3.changed && step4.changed);

    // 2 new at-rule lines + a blank line + 3 lines of new rule.
    assert_eq!(changed_line_count(&source, &step4.source), 6);

    // Everything the file already said is still there, verbatim.
    for line in source.lines() {
        assert!(
            step4.source.contains(line),
            "patching dropped the line {line:?}"
        );
    }

    // And re-running the whole installer is a no-op.
    let again = ensure_at_rule_line(
        &step4.source,
        "@plugin \"@tailwindcss/typography\";",
        opts(),
    )
    .unwrap();
    assert!(!again.changed);
}
