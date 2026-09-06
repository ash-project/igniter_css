// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! The NIF boundary. Deliberately thin.
//!
//! **Elixir sends intent; Rust returns text.** The CST is never exported across
//! the boundary in any form: marshalling trees between languages is costly to
//! maintain and buys nothing here.
//!
//! Two other rules hold throughout this module:
//!
//! * no `unwrap()`/`expect()` on anything reachable from a call -- a panic
//!   takes down a BEAM scheduler, so every fallible path returns
//!   `{:error, _, reason}`;
//! * every NIF is `DirtyCpu`. Input size is unbounded (a vendored `bootstrap.css`
//!   is 200 kB) and the work is pure CPU, so the few microseconds of dirty
//!   scheduling overhead are much cheaper than the risk of blocking a normal
//!   scheduler. See `tests/bench.rs` for the measurements behind that call.

use crate::analyze;
use crate::atoms;
use crate::ctx::ParseOptions;
use crate::error::CssError;
use crate::helpers::encode_response;
use crate::ops::declaration::{Important, SetOptions};
use crate::ops::tidy::DedupeOptions;
use crate::ops::{at_rule, declaration, rule, tidy, Outcome};
use crate::transform;
use rustler::{Env, NifResult, NifStruct, Term};

// ---------------------------------------------------------------------------
// Boundary types
// ---------------------------------------------------------------------------

#[derive(NifStruct, Debug, Clone)]
#[module = "IgniterCss.ParseOpts"]
pub struct ExParseOpts {
    pub allow_wrong_line_comments: bool,
    pub css_modules: bool,
}

impl From<ExParseOpts> for ParseOptions {
    fn from(o: ExParseOpts) -> Self {
        ParseOptions {
            allow_wrong_line_comments: o.allow_wrong_line_comments,
            css_modules: o.css_modules,
        }
    }
}

#[derive(NifStruct, Debug, Clone)]
#[module = "IgniterCss.Outcome"]
pub struct ExOutcome {
    pub source: String,
    pub changed: bool,
    pub diagnostics: Vec<String>,
}

impl From<Outcome> for ExOutcome {
    fn from(o: Outcome) -> Self {
        ExOutcome {
            source: o.source,
            changed: o.changed,
            diagnostics: o.diagnostics,
        }
    }
}

#[derive(NifStruct, Debug, Clone)]
#[module = "IgniterCss.Analysis"]
pub struct ExAnalysis {
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
    pub property_frequency: Vec<(String, usize)>,
    pub selectors: Vec<String>,
    pub at_rule_names: Vec<String>,
}

impl From<analyze::Analysis> for ExAnalysis {
    fn from(a: analyze::Analysis) -> Self {
        ExAnalysis {
            rules_count: a.rules_count,
            top_level_rules_count: a.top_level_rules_count,
            selectors_count: a.selectors_count,
            unique_selectors: a.unique_selectors,
            declarations_count: a.declarations_count,
            unique_properties: a.unique_properties,
            at_rules_count: a.at_rules_count,
            media_queries_count: a.media_queries_count,
            keyframes_count: a.keyframes_count,
            imports_count: a.imports_count,
            comments_count: a.comments_count,
            colors_count: a.colors_count,
            important_count: a.important_count,
            custom_properties_count: a.custom_properties_count,
            property_frequency: a.property_frequency,
            selectors: a.selectors,
            at_rule_names: a.at_rule_names,
        }
    }
}

#[derive(NifStruct, Debug, Clone)]
#[module = "IgniterCss.Validation"]
pub struct ExValidation {
    pub valid: bool,
    pub diagnostics: usize,
    pub round_trips: bool,
    pub message: String,
}

#[derive(NifStruct, Debug, Clone)]
#[module = "IgniterCss.AtRule"]
pub struct ExAtRule {
    pub name: String,
    pub prelude: String,
    pub target: Option<String>,
    pub has_block: bool,
    pub declarations: Vec<(String, String)>,
    pub text: String,
    pub body: Option<String>,
}

#[derive(NifStruct, Debug, Clone)]
#[module = "IgniterCss.Animation"]
pub struct ExAnimation {
    pub name: String,
    pub keyframes: Vec<(String, Vec<(String, String)>)>,
    pub used_by: Vec<String>,
}

// ---------------------------------------------------------------------------
// Plumbing
// ---------------------------------------------------------------------------

fn describe(e: CssError) -> String {
    e.to_string()
}

/// Encode a mutating op's result.
macro_rules! respond {
    ($env:expr, $atom:expr, $call:expr) => {{
        match $call {
            Ok(value) => encode_response($env, atoms::ok(), $atom, value),
            Err(e) => encode_response($env, atoms::error(), $atom, describe(e)),
        }
    }};
}

// ---------------------------------------------------------------------------
// At-rule ops
// ---------------------------------------------------------------------------

#[rustler::nif(schedule = "DirtyCpu")]
fn ensure_at_rule_nif(
    env: Env,
    source: String,
    line: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::ensure_at_rule_nif(),
        at_rule::ensure_at_rule_line(&source, &line, opts.into()).map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn remove_at_rule_nif(
    env: Env,
    source: String,
    name: String,
    matching: Option<String>,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::remove_at_rule_nif(),
        at_rule::remove_at_rule(&source, &name, matching.as_deref(), opts.into())
            .map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ensure_at_rule_block_nif(
    env: Env,
    source: String,
    name: String,
    matching: Option<String>,
    declarations: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::ensure_at_rule_block_nif(),
        at_rule::ensure_at_rule_block(
            &source,
            &name,
            matching.as_deref(),
            &declarations,
            opts.into()
        )
        .map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn has_at_rule_nif(env: Env, source: String, line: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::has_at_rule_nif(),
        at_rule::has_at_rule(&source, &line, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn add_import_nif(
    env: Env,
    source: String,
    url: String,
    media: Option<String>,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::add_import_nif(),
        at_rule::add_import(&source, &url, media.as_deref(), opts.into()).map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn remove_import_nif(env: Env, source: String, url: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::remove_import_nif(),
        at_rule::remove_import(&source, &url, opts.into()).map(ExOutcome::from)
    )
}

// ---------------------------------------------------------------------------
// Rule ops
// ---------------------------------------------------------------------------

#[rustler::nif(schedule = "DirtyCpu")]
fn ensure_rule_nif(
    env: Env,
    source: String,
    selector: String,
    declarations: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::ensure_rule_nif(),
        rule::ensure_rule_with(&source, &selector, &declarations, opts.into()).map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn remove_rule_nif(
    env: Env,
    source: String,
    selector: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::remove_rule_nif(),
        rule::remove_rule(&source, &selector, opts.into()).map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn replace_rule_body_nif(
    env: Env,
    source: String,
    selector: String,
    declarations: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::replace_rule_body_nif(),
        rule::replace_rule_body(&source, &selector, &declarations, opts.into())
            .map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn append_raw_to_rule_nif(
    env: Env,
    source: String,
    selector: String,
    raw: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::append_raw_to_rule_nif(),
        rule::append_raw_to_rule(&source, &selector, &raw, opts.into()).map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn has_rule_nif(env: Env, source: String, selector: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::has_rule_nif(),
        rule::has_rule(&source, &selector, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn list_selectors_nif(env: Env, source: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::list_selectors_nif(),
        rule::list_selectors(&source, opts.into())
    )
}

// ---------------------------------------------------------------------------
// Declaration ops
// ---------------------------------------------------------------------------

#[rustler::nif(schedule = "DirtyCpu")]
#[allow(clippy::too_many_arguments)]
fn set_declaration_nif(
    env: Env,
    source: String,
    selector: String,
    property: String,
    value: String,
    important: Option<bool>,
    create_rule: bool,
    opts: ExParseOpts,
) -> NifResult<Term> {
    let set = SetOptions {
        important: Important::from_option(important),
        create_rule,
    };
    respond!(
        env,
        atoms::set_declaration_nif(),
        declaration::set_declaration(&source, &selector, &property, &value, set, opts.into())
            .map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn remove_declaration_nif(
    env: Env,
    source: String,
    selector: String,
    property: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::remove_declaration_nif(),
        declaration::remove_declaration(&source, &selector, &property, opts.into())
            .map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn get_declaration_nif(
    env: Env,
    source: String,
    selector: String,
    property: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::get_declaration_nif(),
        declaration::get_declaration(&source, &selector, &property, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn has_declaration_nif(
    env: Env,
    source: String,
    selector: String,
    property: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::has_declaration_nif(),
        declaration::has_declaration(&source, &selector, &property, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn get_rule_declarations_nif(
    env: Env,
    source: String,
    selector: String,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::get_rule_declarations_nif(),
        declaration::get_rule_declarations(&source, &selector, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn add_vendor_prefixes_nif(
    env: Env,
    source: String,
    property: String,
    prefixes: Vec<String>,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::add_vendor_prefixes_nif(),
        declaration::add_vendor_prefixes(&source, &property, &prefixes, opts.into())
            .map(ExOutcome::from)
    )
}

// ---------------------------------------------------------------------------
// Tidy ops
// ---------------------------------------------------------------------------

#[rustler::nif(schedule = "DirtyCpu")]
fn sort_properties_nif(env: Env, source: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::sort_properties_nif(),
        tidy::sort_properties(&source, opts.into()).map(ExOutcome::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn remove_duplicates_nif(
    env: Env,
    source: String,
    declarations: bool,
    rules: bool,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::remove_duplicates_nif(),
        tidy::remove_duplicates(
            &source,
            DedupeOptions {
                declarations,
                rules
            },
            opts.into()
        )
        .map(ExOutcome::from)
    )
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

#[rustler::nif(schedule = "DirtyCpu")]
fn analyze_nif(env: Env, source: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::analyze_nif(),
        analyze::analyze(&source, opts.into()).map(ExAnalysis::from)
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn validate_nif(env: Env, source: String, opts: ExParseOpts) -> NifResult<Term> {
    let v = analyze::validate(&source, opts.into());
    let status = if v.valid { atoms::ok() } else { atoms::error() };
    let payload = ExValidation {
        valid: v.valid,
        diagnostics: v.diagnostics,
        round_trips: v.round_trips,
        message: v.message,
    };
    encode_response(env, status, atoms::validate_nif(), payload)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn extract_colors_nif(env: Env, source: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::extract_colors_nif(),
        analyze::extract_colors(&source, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn extract_media_queries_nif(env: Env, source: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::extract_media_queries_nif(),
        analyze::extract_media_queries(&source, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn extract_animations_nif(env: Env, source: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::extract_animations_nif(),
        analyze::extract_animations(&source, opts.into()).map(|list| {
            list.into_iter()
                .map(|a| ExAnimation {
                    name: a.name,
                    keyframes: a.keyframes,
                    used_by: a.used_by,
                })
                .collect::<Vec<_>>()
        })
    )
}

// ---------------------------------------------------------------------------
// Whole-file transforms
// ---------------------------------------------------------------------------

#[rustler::nif(schedule = "DirtyCpu")]
fn minify_nif(env: Env, source: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::minify_nif(),
        transform::minify(&source, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn beautify_nif(env: Env, source: String, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::beautify_nif(),
        transform::beautify(&source, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn merge_stylesheets_nif(env: Env, sources: Vec<String>, opts: ExParseOpts) -> NifResult<Term> {
    respond!(
        env,
        atoms::merge_stylesheets_nif(),
        transform::merge_stylesheets(&sources, opts.into())
    )
}

#[rustler::nif(schedule = "DirtyCpu")]
fn get_at_rules_nif(
    env: Env,
    source: String,
    name: String,
    matching: Option<String>,
    opts: ExParseOpts,
) -> NifResult<Term> {
    respond!(
        env,
        atoms::get_at_rules_nif(),
        analyze::get_at_rules(&source, &name, matching.as_deref(), opts.into()).map(|list| {
            list.into_iter()
                .map(|a| ExAtRule {
                    name: a.name,
                    prelude: a.prelude,
                    target: a.target,
                    has_block: a.has_block,
                    declarations: a.declarations,
                    text: a.text,
                    body: a.body,
                })
                .collect::<Vec<_>>()
        })
    )
}

rustler::init!("Elixir.IgniterCss.Native");
