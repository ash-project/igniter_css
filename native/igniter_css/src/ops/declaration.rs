// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! Declaration-level codemods: the ops Igniter installers reach for most.

use crate::ctx::ParseOptions;
use crate::edit::Edit;
use crate::error::{CssError, Result};
use crate::locate::{
    declaration_lists, declarations_in, find_declaration, find_declarations, normalize_property,
};
use crate::ops::rule::{append_rule_edits, append_to_body, resolve_rule};
use crate::ops::{query, run, validate_snippet, Outcome};
use crate::trivia::{comment_ranges, deletion_span};

/// What to do with the `!important` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Important {
    /// Leave whatever the declaration already has.
    #[default]
    Keep,
    Set,
    Unset,
}

impl Important {
    pub fn from_option(value: Option<bool>) -> Self {
        match value {
            None => Self::Keep,
            Some(true) => Self::Set,
            Some(false) => Self::Unset,
        }
    }

    fn resolve(self, current: bool) -> bool {
        match self {
            Self::Keep => current,
            Self::Set => true,
            Self::Unset => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SetOptions {
    pub important: Important,
    /// Create `selector { property: value; }` when the rule does not exist.
    /// Without it, a missing rule is a `NotFound` error.
    pub create_rule: bool,
}

fn check_property_and_value(property: &str, value: &str) -> Result<(String, String)> {
    let property = property.trim();
    let value = value.trim();
    if property.is_empty() {
        return Err(CssError::InvalidInput("property name is empty".to_string()));
    }
    if value.is_empty() {
        return Err(CssError::InvalidInput("value is empty".to_string()));
    }
    if property.contains([':', ';', '{', '}']) {
        return Err(CssError::InvalidInput(format!(
            "property name {property:?} contains a delimiter"
        )));
    }
    validate_snippet(value, "value")?;
    if value.contains('{') || value.contains('}') {
        return Err(CssError::InvalidInput(format!(
            "value {value:?} must not contain braces"
        )));
    }
    // A `;` outside of a string, comment or `url(...)` would silently turn one
    // declaration into two.
    if crate::ops::split_declarations(value).len() > 1 {
        return Err(CssError::InvalidInput(format!(
            "value {value:?} contains a `;`; pass one declaration at a time"
        )));
    }
    // The flag is controlled by `SetOptions::important`, not by the value text.
    if value.to_lowercase().contains("!important") {
        return Err(CssError::InvalidInput(
            "put `!important` in the options, not in the value".to_string(),
        ));
    }
    Ok((property.to_string(), value.to_string()))
}

/// Update `property` inside the rule matching `selector`, or append it.
///
/// When the property is already present only its **value** bytes are replaced
/// (rule E), so an inline comment on that line and any `!important` the caller
/// did not ask to change both survive untouched.
pub fn set_declaration(
    source: &str,
    selector: &str,
    property: &str,
    value: &str,
    set: SetOptions,
    options: ParseOptions,
) -> Result<Outcome> {
    let (property, value) = check_property_and_value(property, value)?;

    run(source, options, |ctx| {
        let Some(rule) = resolve_rule(ctx, selector)? else {
            if !set.create_rule {
                return Err(CssError::NotFound(format!(
                    "no top-level rule with selector {selector:?}"
                )));
            }
            let suffix = if set.important == Important::Set {
                " !important"
            } else {
                ""
            };
            let decl = format!("{property}: {value}{suffix};");
            return Ok(append_rule_edits(ctx, selector.trim(), &[decl]));
        };

        match find_declaration(ctx, &rule, &property) {
            Some(d) => {
                let want_important = set.important.resolve(d.important);
                if want_important == d.important {
                    // Value bytes only -- the minimal possible diff.
                    Ok(vec![Edit::replace(d.value_start, d.value_end, value)])
                } else if want_important {
                    Ok(vec![Edit::replace(
                        d.value_start,
                        d.value_end,
                        format!("{value} !important"),
                    )])
                } else {
                    let (_, imp_end) = d
                        .important_range
                        .expect("important flag present when d.important");
                    Ok(vec![Edit::replace(d.value_start, imp_end, value)])
                }
            }
            None => {
                let suffix = if set.important == Important::Set {
                    " !important"
                } else {
                    ""
                };
                let decl = format!("{property}: {value}{suffix};");
                Ok(append_to_body(ctx, &rule, &decl))
            }
        }
    })
}

/// Remove every declaration of `property` from the rule matching `selector`,
/// together with the comments those declarations own (rule D).
pub fn remove_declaration(
    source: &str,
    selector: &str,
    property: &str,
    options: ParseOptions,
) -> Result<Outcome> {
    let property = property.trim().to_string();
    if property.is_empty() {
        return Err(CssError::InvalidInput("property name is empty".to_string()));
    }

    run(source, options, |ctx| {
        let Some(rule) = resolve_rule(ctx, selector)? else {
            // Nothing to remove from a rule that isn't there.
            return Ok(vec![]);
        };
        let comments = comment_ranges(ctx);
        Ok(find_declarations(ctx, &rule, &property)
            .into_iter()
            .map(|d| {
                let span = deletion_span(ctx, &comments, d.start, d.end);
                Edit::delete(span.start, span.end)
            })
            .collect())
    })
}

/// Add vendor-prefixed copies of `property` next to every occurrence of it,
/// anywhere in the file.
///
/// Each prefixed declaration is inserted immediately **before** the unprefixed
/// one, which is the ordering browsers expect: the standard property wins.
/// Prefixes already present in the same block are skipped, so re-running the op
/// is a no-op (rule A).
pub fn add_vendor_prefixes(
    source: &str,
    property: &str,
    prefixes: &[String],
    options: ParseOptions,
) -> Result<Outcome> {
    let property = normalize_property(property);
    if property.is_empty() {
        return Err(CssError::InvalidInput("property name is empty".to_string()));
    }
    for p in prefixes {
        if p.trim().is_empty() || p.contains([':', ';', '{', '}']) {
            return Err(CssError::InvalidInput(format!("invalid prefix {p:?}")));
        }
    }
    if prefixes.is_empty() {
        return Ok(Outcome::unchanged(source));
    }

    run(source, options, |ctx| {
        let nl = ctx.nl();
        let mut edits = Vec::new();

        for (_, decls) in declaration_lists(ctx) {
            let present: Vec<String> = decls
                .iter()
                .map(|d| normalize_property(&d.property))
                .collect();

            for d in &decls {
                if normalize_property(&d.property) != property {
                    continue;
                }
                let mut lines = Vec::new();
                for prefix in prefixes {
                    let prefixed = normalize_property(&format!("{}{property}", prefix.trim()));
                    if present.contains(&prefixed) {
                        continue;
                    }
                    let flag = if d.important { " !important" } else { "" };
                    lines.push(format!(
                        "{prefixed}: {}{flag};",
                        ctx.source()[d.value_start..d.value_end].trim()
                    ));
                }
                if lines.is_empty() {
                    continue;
                }

                // Insert at the start of the declaration's own line so the new
                // lines inherit its indentation exactly.
                if ctx.is_at_line_start(d.start) {
                    let indent = ctx.indent_at(d.start);
                    let text = lines
                        .iter()
                        .map(|l| format!("{indent}{l}{nl}"))
                        .collect::<String>();
                    edits.push(Edit::insert(ctx.line_start(d.start), text));
                } else {
                    let text = format!("{} ", lines.join(" "));
                    edits.push(Edit::insert(d.start, text));
                }
            }
        }
        Ok(edits)
    })
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// The value of `property` in the rule matching `selector`, as written.
pub fn get_declaration(
    source: &str,
    selector: &str,
    property: &str,
    options: ParseOptions,
) -> Result<Option<String>> {
    query(source, options, |ctx| {
        let Some(rule) = resolve_rule(ctx, selector)? else {
            return Ok(None);
        };
        Ok(find_declaration(ctx, &rule, property).map(|d| {
            if d.important {
                format!("{} !important", d.value_raw.trim())
            } else {
                d.value_raw.trim().to_string()
            }
        }))
    })
}

pub fn has_declaration(
    source: &str,
    selector: &str,
    property: &str,
    options: ParseOptions,
) -> Result<bool> {
    Ok(get_declaration(source, selector, property, options)?.is_some())
}

/// Every declaration in the rule matching `selector`, as `(property, value)`
/// pairs in source order.
pub fn get_rule_declarations(
    source: &str,
    selector: &str,
    options: ParseOptions,
) -> Result<Option<Vec<(String, String)>>> {
    query(source, options, |ctx| {
        let Some(rule) = resolve_rule(ctx, selector)? else {
            return Ok(None);
        };
        Ok(Some(
            declarations_in(ctx, &rule)
                .into_iter()
                .map(|d| {
                    let value = if d.important {
                        format!("{} !important", d.value_raw.trim())
                    } else {
                        d.value_raw.trim().to_string()
                    };
                    (d.property, value)
                })
                .collect(),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> ParseOptions {
        ParseOptions::default()
    }

    fn set(src: &str, sel: &str, prop: &str, val: &str) -> Outcome {
        set_declaration(src, sel, prop, val, SetOptions::default(), opts()).unwrap()
    }

    fn set_with(src: &str, sel: &str, prop: &str, val: &str, o: SetOptions) -> Outcome {
        set_declaration(src, sel, prop, val, o, opts()).unwrap()
    }

    fn remove(src: &str, sel: &str, prop: &str) -> Outcome {
        remove_declaration(src, sel, prop, opts()).unwrap()
    }

    // -- set: updating an existing declaration ------------------------------

    #[test]
    fn updates_an_existing_value() {
        let o = set(".a {\n  color: red;\n}\n", ".a", "color", "blue");
        assert!(o.changed);
        assert_eq!(o.source, ".a {\n  color: blue;\n}\n");
    }

    #[test]
    fn updating_touches_only_the_value_bytes() {
        let src = ".a {\n  color: red; /* the brand */\n  margin: 0;\n}\n";
        let o = set(src, ".a", "color", "blue");
        assert_eq!(
            o.source,
            ".a {\n  color: blue; /* the brand */\n  margin: 0;\n}\n"
        );
    }

    #[test]
    fn updating_preserves_an_existing_important_flag() {
        let src = ".a { color: red !important; }\n";
        let o = set(src, ".a", "color", "blue");
        assert_eq!(o.source, ".a { color: blue !important; }\n");
    }

    #[test]
    fn can_add_an_important_flag() {
        let src = ".a { color: red; }\n";
        let o = set_with(
            src,
            ".a",
            "color",
            "blue",
            SetOptions {
                important: Important::Set,
                ..Default::default()
            },
        );
        assert_eq!(o.source, ".a { color: blue !important; }\n");
    }

    #[test]
    fn can_remove_an_important_flag() {
        let src = ".a { color: red   !important; }\n";
        let o = set_with(
            src,
            ".a",
            "color",
            "blue",
            SetOptions {
                important: Important::Unset,
                ..Default::default()
            },
        );
        assert_eq!(o.source, ".a { color: blue; }\n");
    }

    #[test]
    fn updates_a_custom_property() {
        let src = ":root {\n  --brand: #fff;\n}\n";
        let o = set(src, ":root", "--brand", "#000");
        assert_eq!(o.source, ":root {\n  --brand: #000;\n}\n");
    }

    #[test]
    fn updates_the_last_of_a_repeated_property() {
        let src = ".a {\n  color: red;\n  color: green;\n}\n";
        let o = set(src, ".a", "color", "blue");
        assert_eq!(o.source, ".a {\n  color: red;\n  color: blue;\n}\n");
    }

    #[test]
    fn a_multi_token_value_is_replaced_whole() {
        let src = ".a { margin: 0 auto 10px; }\n";
        let o = set(src, ".a", "margin", "1rem");
        assert_eq!(o.source, ".a { margin: 1rem; }\n");
    }

    #[test]
    fn accepts_a_function_value() {
        let o = set(".a { color: red; }\n", ".a", "color", "var(--brand, #fff)");
        assert_eq!(o.source, ".a { color: var(--brand, #fff); }\n");
    }

    #[test]
    fn accepts_a_url_value_containing_a_semicolon() {
        let o = set(
            ".a { background: none; }\n",
            ".a",
            "background",
            "url(data:image/svg+xml;base64,AA==)",
        );
        assert_eq!(
            o.source,
            ".a { background: url(data:image/svg+xml;base64,AA==); }\n"
        );
    }

    // -- set: appending a new declaration -----------------------------------

    #[test]
    fn appends_a_missing_property() {
        let src = ".a {\n  color: red;\n}\n";
        let o = set(src, ".a", "margin", "0");
        assert_eq!(o.source, ".a {\n  color: red;\n  margin: 0;\n}\n");
    }

    #[test]
    fn appends_into_an_empty_rule() {
        let o = set(".a {\n}\n", ".a", "color", "red");
        assert_eq!(o.source, ".a {\n  color: red;\n}\n");
    }

    #[test]
    fn appends_inline_for_a_single_line_rule() {
        let o = set(".a { color: red; }\n", ".a", "margin", "0");
        assert_eq!(o.source, ".a { color: red; margin: 0; }\n");
    }

    #[test]
    fn appending_matches_the_files_indentation() {
        let src = ".a {\n    color: red;\n}\n";
        let o = set(src, ".a", "margin", "0");
        assert_eq!(o.source, ".a {\n    color: red;\n    margin: 0;\n}\n");
    }

    #[test]
    fn appending_uses_tabs_when_the_file_does() {
        let src = ".a {\n\tcolor: red;\n}\n";
        let o = set(src, ".a", "margin", "0");
        assert_eq!(o.source, ".a {\n\tcolor: red;\n\tmargin: 0;\n}\n");
    }

    #[test]
    fn appending_uses_crlf_when_the_file_does() {
        let src = ".a {\r\n  color: red;\r\n}\r\n";
        let o = set(src, ".a", "margin", "0");
        assert_eq!(o.source, ".a {\r\n  color: red;\r\n  margin: 0;\r\n}\r\n");
    }

    #[test]
    fn appending_lands_after_a_trailing_comment() {
        let src = ".a {\n  color: red; /* note */\n}\n";
        let o = set(src, ".a", "margin", "0");
        assert_eq!(
            o.source,
            ".a {\n  color: red; /* note */\n  margin: 0;\n}\n"
        );
    }

    #[test]
    fn appending_terminates_the_previous_declaration() {
        let src = ".a {\n  color: red\n}\n";
        let o = set(src, ".a", "margin", "0");
        assert_eq!(o.source, ".a {\n  color: red;\n  margin: 0;\n}\n");
    }

    #[test]
    fn appending_keeps_a_dangling_comment_at_the_end_of_the_body() {
        let src = ".a {\n  color: red;\n  /* end of block */\n}\n";
        let o = set(src, ".a", "margin", "0");
        assert!(o.source.contains("/* end of block */"));
    }

    // -- set: missing rules -------------------------------------------------

    #[test]
    fn a_missing_rule_is_an_error_by_default() {
        let e = set_declaration(
            ".a {}\n",
            ".zz",
            "color",
            "red",
            SetOptions::default(),
            opts(),
        )
        .unwrap_err();
        assert!(matches!(e, CssError::NotFound(_)));
    }

    #[test]
    fn a_missing_rule_can_be_created_on_request() {
        let o = set_with(
            ".a {}\n",
            ".hide-scrollbar",
            "display",
            "none",
            SetOptions {
                create_rule: true,
                ..Default::default()
            },
        );
        assert_eq!(
            o.source,
            ".a {}\n\n.hide-scrollbar {\n  display: none;\n}\n"
        );
    }

    #[test]
    fn an_ambiguous_selector_is_an_error() {
        let e = set_declaration(
            ".a {}\n.a {}\n",
            ".a",
            "color",
            "red",
            SetOptions::default(),
            opts(),
        )
        .unwrap_err();
        assert!(matches!(e, CssError::AmbiguousSelector { count: 2, .. }));
    }

    // -- set: idempotency and validation ------------------------------------

    #[test]
    fn set_is_idempotent() {
        let src = ".a {\n  color: red;\n}\n";
        let once = set(src, ".a", "margin", "0");
        let twice = set(&once.source, ".a", "margin", "0");
        assert!(once.changed);
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn writing_back_the_same_value_reports_no_change() {
        let src = ".a {\n  color: red;\n}\n";
        let o = set(src, ".a", "color", "red");
        assert!(!o.changed);
        assert_eq!(o.source, src);
    }

    #[test]
    fn rejects_a_value_carrying_its_own_delimiters() {
        for (prop, value) in [
            ("color", "red; margin: 0"),
            ("color", "red !important"),
            ("color:x", "red"),
            ("", "red"),
            ("color", "  "),
            ("color", "red } .b {"),
        ] {
            assert!(
                set_declaration(".a{}", ".a", prop, value, SetOptions::default(), opts()).is_err(),
                "should have rejected {prop:?}: {value:?}"
            );
        }
    }

    // -- remove -------------------------------------------------------------

    #[test]
    fn removes_a_declaration_and_its_line() {
        let src = ".a {\n  color: red;\n  margin: 0;\n}\n";
        let o = remove(src, ".a", "color");
        assert!(o.changed);
        assert_eq!(o.source, ".a {\n  margin: 0;\n}\n");
    }

    #[test]
    fn removing_takes_the_trailing_comment_with_it() {
        let src = ".a {\n  color: red; /* legacy */\n  margin: 0;\n}\n";
        let o = remove(src, ".a", "color");
        assert_eq!(o.source, ".a {\n  margin: 0;\n}\n");
    }

    #[test]
    fn removing_takes_an_adjacent_comment_above() {
        let src = ".a {\n  /* brand */\n  color: red;\n  margin: 0;\n}\n";
        let o = remove(src, ".a", "color");
        assert_eq!(o.source, ".a {\n  margin: 0;\n}\n");
    }

    #[test]
    fn removing_keeps_a_comment_separated_by_a_blank_line() {
        let src = ".a {\n  /* about the block */\n\n  color: red;\n  margin: 0;\n}\n";
        let o = remove(src, ".a", "color");
        assert_eq!(
            o.source,
            ".a {\n  /* about the block */\n\n  margin: 0;\n}\n"
        );
    }

    #[test]
    fn removing_keeps_a_section_header() {
        let src = ".a {\n  /* ===== colours ===== */\n  color: red;\n  margin: 0;\n}\n";
        let o = remove(src, ".a", "color");
        assert_eq!(
            o.source,
            ".a {\n  /* ===== colours ===== */\n  margin: 0;\n}\n"
        );
    }

    #[test]
    fn removes_every_copy_of_a_repeated_property() {
        let src = ".a {\n  color: red;\n  margin: 0;\n  color: blue;\n}\n";
        let o = remove(src, ".a", "color");
        assert_eq!(o.source, ".a {\n  margin: 0;\n}\n");
    }

    #[test]
    fn removing_an_absent_property_is_a_no_op() {
        let src = ".a {\n  color: red;\n}\n";
        let o = remove(src, ".a", "margin");
        assert!(!o.changed);
        assert_eq!(o.source, src);
    }

    #[test]
    fn removing_from_an_absent_rule_is_a_no_op() {
        let src = ".a {\n  color: red;\n}\n";
        let o = remove(src, ".zz", "color");
        assert!(!o.changed);
    }

    #[test]
    fn remove_is_idempotent() {
        let src = ".a {\n  color: red;\n  margin: 0;\n}\n";
        let once = remove(src, ".a", "color");
        let twice = remove(&once.source, ".a", "color");
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn removing_the_only_declaration_leaves_an_empty_rule() {
        let src = ".a {\n  color: red;\n}\n";
        let o = remove(src, ".a", "color");
        assert_eq!(o.source, ".a {\n}\n");
    }

    // -- vendor prefixes ----------------------------------------------------

    fn prefixes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn adds_vendor_prefixes_above_the_standard_property() {
        let src = ".a {\n  user-select: none;\n}\n";
        let o = add_vendor_prefixes(
            src,
            "user-select",
            &prefixes(&["-webkit-", "-moz-"]),
            opts(),
        )
        .unwrap();
        assert_eq!(
            o.source,
            ".a {\n  -webkit-user-select: none;\n  -moz-user-select: none;\n  user-select: none;\n}\n"
        );
    }

    #[test]
    fn prefixes_every_occurrence_in_the_file() {
        let src =
            ".a { user-select: none; }\n@media print {\n  .b {\n    user-select: text;\n  }\n}\n";
        let o = add_vendor_prefixes(src, "user-select", &prefixes(&["-webkit-"]), opts()).unwrap();
        assert!(o.source.contains("-webkit-user-select: none;"));
        assert!(o.source.contains("-webkit-user-select: text;"));
    }

    #[test]
    fn does_nothing_when_the_property_is_absent() {
        let src = ".a {\n  color: red;\n}\n";
        let o = add_vendor_prefixes(src, "user-select", &prefixes(&["-webkit-"]), opts()).unwrap();
        assert!(!o.changed);
        assert_eq!(o.source, src);
    }

    #[test]
    fn an_empty_prefix_list_is_a_no_op() {
        let src = ".a {\n  user-select: none;\n}\n";
        let o = add_vendor_prefixes(src, "user-select", &[], opts()).unwrap();
        assert!(!o.changed);
    }

    #[test]
    fn vendor_prefixes_are_idempotent() {
        let src = ".a {\n  user-select: none;\n}\n";
        let once = add_vendor_prefixes(
            src,
            "user-select",
            &prefixes(&["-webkit-", "-moz-"]),
            opts(),
        )
        .unwrap();
        let twice = add_vendor_prefixes(
            &once.source,
            "user-select",
            &prefixes(&["-webkit-", "-moz-"]),
            opts(),
        )
        .unwrap();
        assert!(once.changed);
        assert!(!twice.changed);
        assert_eq!(once.source, twice.source);
    }

    #[test]
    fn an_already_present_prefix_is_skipped() {
        let src = ".a {\n  -webkit-user-select: none;\n  user-select: none;\n}\n";
        let o = add_vendor_prefixes(
            src,
            "user-select",
            &prefixes(&["-webkit-", "-moz-"]),
            opts(),
        )
        .unwrap();
        assert_eq!(
            o.source,
            ".a {\n  -webkit-user-select: none;\n  -moz-user-select: none;\n  user-select: none;\n}\n"
        );
    }

    #[test]
    fn prefixed_copies_carry_the_important_flag() {
        let src = ".a {\n  user-select: none !important;\n}\n";
        let o = add_vendor_prefixes(src, "user-select", &prefixes(&["-webkit-"]), opts()).unwrap();
        assert!(o.source.contains("-webkit-user-select: none !important;"));
    }

    #[test]
    fn prefixing_preserves_comments() {
        let src = ".a {\n  /* no text selection */\n  user-select: none; /* everywhere */\n}\n";
        let o = add_vendor_prefixes(src, "user-select", &prefixes(&["-webkit-"]), opts()).unwrap();
        assert_eq!(
            o.source,
            ".a {\n  /* no text selection */\n  -webkit-user-select: none;\n  user-select: none; /* everywhere */\n}\n"
        );
    }

    #[test]
    fn prefixing_a_single_line_rule_stays_inline() {
        let src = ".a { user-select: none; }\n";
        let o = add_vendor_prefixes(src, "user-select", &prefixes(&["-webkit-"]), opts()).unwrap();
        assert_eq!(
            o.source,
            ".a { -webkit-user-select: none; user-select: none; }\n"
        );
    }

    #[test]
    fn rejects_a_malformed_prefix() {
        assert!(add_vendor_prefixes(".a{}", "x", &prefixes(&["a;b"]), opts()).is_err());
        assert!(add_vendor_prefixes(".a{}", "", &prefixes(&["-webkit-"]), opts()).is_err());
    }

    // -- queries ------------------------------------------------------------

    #[test]
    fn reads_a_declaration_value() {
        let src = ".a {\n  color: red;\n}\n";
        assert_eq!(
            get_declaration(src, ".a", "color", opts()).unwrap(),
            Some("red".to_string())
        );
        assert_eq!(get_declaration(src, ".a", "margin", opts()).unwrap(), None);
        assert_eq!(get_declaration(src, ".zz", "color", opts()).unwrap(), None);
    }

    #[test]
    fn reads_a_value_with_its_important_flag() {
        let src = ".a { color: red !important; }\n";
        assert_eq!(
            get_declaration(src, ".a", "color", opts()).unwrap(),
            Some("red !important".to_string())
        );
    }

    #[test]
    fn has_declaration_agrees_with_get() {
        let src = ".a { color: red; }\n";
        assert!(has_declaration(src, ".a", "color", opts()).unwrap());
        assert!(!has_declaration(src, ".a", "margin", opts()).unwrap());
    }

    #[test]
    fn reads_all_declarations_of_a_rule() {
        let src = ".a {\n  color: red;\n  margin: 0 auto;\n}\n";
        assert_eq!(
            get_rule_declarations(src, ".a", opts()).unwrap(),
            Some(vec![
                ("color".into(), "red".into()),
                ("margin".into(), "0 auto".into()),
            ])
        );
        assert_eq!(get_rule_declarations(src, ".zz", opts()).unwrap(), None);
    }
}
