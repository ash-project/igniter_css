// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! `ParseCtx` owns the source string and its lossless parse, plus the handful
//! of formatting facts every codemod needs so that inserted text looks like the
//! text the user already wrote (ROADMAP Phase 1, and rules B/C in §8).
//!
//! This module and `locate` are the only two places allowed to name Biome
//! types. Isolating them here is the mitigation for R2 (Biome API churn).

use biome_css_parser::{parse_css, CssParse, CssParserOptions};
use biome_css_syntax::{CssSyntaxKind, CssSyntaxNode};
use biome_rowan::TextRange;

pub const BOM: &str = "\u{feff}";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Newline {
    Lf,
    CrLf,
}

impl Newline {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Treat `//` as a comment. Off by default, matching Biome.
    pub allow_wrong_line_comments: bool,
    pub css_modules: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            // css-in-js habits leak `//` into real .css files often enough that
            // tolerating them by default is the friendlier behaviour, and it
            // cannot lose data: with the flag off those bytes become bogus
            // nodes, with it on they become comments. Either way they survive.
            allow_wrong_line_comments: true,
            css_modules: false,
        }
    }
}

impl ParseOptions {
    pub fn strict() -> Self {
        Self {
            allow_wrong_line_comments: false,
            css_modules: false,
        }
    }

    fn to_biome(self) -> CssParserOptions {
        let mut o = CssParserOptions::default();
        if self.allow_wrong_line_comments {
            o = o.allow_wrong_line_comments();
        }
        if self.css_modules {
            o = o.allow_css_modules();
        }
        o
    }
}

pub struct ParseCtx {
    source: String,
    parse: CssParse,
    newline: Newline,
    indent: String,
    has_final_newline: bool,
    has_bom: bool,
}

impl ParseCtx {
    /// Note on the BOM: Biome lexes a leading U+FEFF as part of the first
    /// identifier, which turns `.a` into a type selector named "\u{feff}" and
    /// would silently break every selector match on a BOM'd file. So we strip
    /// it here, work on BOM-less text throughout, and re-attach it in
    /// `restore_bom` on the way out. Every offset in this crate is therefore an
    /// offset into the BOM-less source.
    pub fn new(source: impl Into<String>, options: ParseOptions) -> Self {
        let raw = source.into();
        let has_bom = raw.starts_with(BOM);
        let source = if has_bom {
            raw[BOM.len()..].to_string()
        } else {
            raw
        };
        let parse = parse_css(&source, options.to_biome());
        let newline = detect_newline(&source);
        let indent = detect_indent(&source);
        let has_final_newline = source.ends_with('\n');
        Self {
            source,
            parse,
            newline,
            indent,
            has_final_newline,
            has_bom,
        }
    }

    pub fn parse_default(source: impl Into<String>) -> Self {
        Self::new(source, ParseOptions::default())
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn syntax(&self) -> CssSyntaxNode {
        self.parse.syntax()
    }

    pub fn diagnostics_count(&self) -> usize {
        self.parse.diagnostics().len()
    }

    pub fn has_errors(&self) -> bool {
        self.parse.has_errors()
    }

    /// The parse is lossless by construction, but assert it before we let any
    /// codemod compute offsets against it. If this ever fails, the byte-range
    /// design's foundation is gone and we must refuse to patch (hard constraint
    /// #4: never destroy input).
    pub fn round_trips(&self) -> bool {
        self.parse.syntax().to_string() == self.source
    }

    pub fn newline(&self) -> Newline {
        self.newline
    }

    pub fn nl(&self) -> &'static str {
        self.newline.as_str()
    }

    /// The file's indent unit -- one level, e.g. `"  "` or `"\t"`.
    pub fn indent(&self) -> &str {
        &self.indent
    }

    pub fn has_final_newline(&self) -> bool {
        self.has_final_newline
    }

    pub fn has_bom(&self) -> bool {
        self.has_bom
    }

    /// Put the BOM back on a result produced from `self.source()`.
    pub fn restore_bom(&self, out: String) -> String {
        if self.has_bom {
            let mut s = String::with_capacity(BOM.len() + out.len());
            s.push_str(BOM);
            s.push_str(&out);
            s
        } else {
            out
        }
    }

    /// Are `{` and `}` balanced across the whole file?
    ///
    /// Checked against the token stream, so braces inside strings and comments
    /// do not count. An unbalanced file cannot be patched safely: text inserted
    /// "at the top level" would land inside somebody's unterminated block, and
    /// hard constraint #4 says a wrong patch is far worse than no patch.
    pub fn braces_are_balanced(&self) -> bool {
        let mut depth = 0i32;
        for token in self
            .parse
            .syntax()
            .descendants_tokens(biome_rowan::Direction::Next)
        {
            match token.kind() {
                CssSyntaxKind::L_CURLY => depth += 1,
                CssSyntaxKind::R_CURLY => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }
        depth == 0
    }

    pub fn text(&self, range: TextRange) -> &str {
        let start = usize::from(range.start());
        let end = usize::from(range.end());
        self.source.get(start..end).unwrap_or("")
    }

    /// Byte offset of the start of the line containing `offset`.
    pub fn line_start(&self, offset: usize) -> usize {
        self.source[..offset]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    /// Byte offset just past the newline terminating the line containing
    /// `offset`, or EOF.
    pub fn line_end_inclusive(&self, offset: usize) -> usize {
        match self.source[offset..].find('\n') {
            Some(i) => offset + i + 1,
            None => self.source.len(),
        }
    }

    /// Leading whitespace of the line containing `offset` -- the indentation to
    /// copy when inserting a sibling next to it (rule B).
    pub fn indent_at(&self, offset: usize) -> &str {
        let start = self.line_start(offset);
        let line = &self.source[start..];
        let ws = line
            .find(|c: char| c != ' ' && c != '\t')
            .unwrap_or(line.len());
        &line[..ws]
    }

    /// Everything between the start of the line and `offset` is whitespace.
    pub fn is_at_line_start(&self, offset: usize) -> bool {
        self.source[self.line_start(offset)..offset]
            .chars()
            .all(|c| c == ' ' || c == '\t')
    }

    /// Everything between `offset` and the end of the line is whitespace.
    pub fn is_at_line_end(&self, offset: usize) -> bool {
        let rest = &self.source[offset..];
        let upto = rest.find('\n').unwrap_or(rest.len());
        rest[..upto]
            .chars()
            .all(|c| c == ' ' || c == '\t' || c == '\r')
    }
}

fn detect_newline(source: &str) -> Newline {
    match (source.find("\r\n"), source.find('\n')) {
        // The first newline in the file wins. `find("\r\n")` points at the CR,
        // so an equal-or-earlier position means the first LF is part of a CRLF.
        (Some(crlf), Some(lf)) if crlf + 1 == lf => Newline::CrLf,
        _ => Newline::Lf,
    }
}

/// Infer one indent level from the file's own declarations.
///
/// We look at the leading whitespace of lines that sit inside a block and take
/// the most common non-empty prefix. Tabs win outright if any indented line
/// uses them, since mixing is worse than guessing the width wrong.
fn detect_indent(source: &str) -> String {
    let mut saw_tab = false;
    let mut widths: Vec<usize> = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if trimmed.is_empty() {
            continue;
        }
        let ws = &line[..line.len() - trimmed.len()];
        if ws.is_empty() {
            continue;
        }
        if ws.contains('\t') {
            saw_tab = true;
        } else {
            widths.push(ws.len());
        }
    }

    if saw_tab {
        return "\t".to_string();
    }

    // The smallest indentation width present is one level.
    match widths.iter().copied().min() {
        Some(n) if n > 0 => " ".repeat(n),
        _ => "  ".to_string(),
    }
}

/// True for the kinds that make up a comment trivia piece.
pub fn is_comment_kind(kind: biome_rowan::TriviaPieceKind) -> bool {
    matches!(
        kind,
        biome_rowan::TriviaPieceKind::SingleLineComment
            | biome_rowan::TriviaPieceKind::MultiLineComment
    )
}

/// Kinds that represent a rule Biome could not understand. They still carry
/// their original source text, which is exactly why we can patch around them.
pub fn is_bogus(kind: CssSyntaxKind) -> bool {
    matches!(
        kind,
        CssSyntaxKind::CSS_BOGUS
            | CssSyntaxKind::CSS_BOGUS_RULE
            | CssSyntaxKind::CSS_BOGUS_AT_RULE
            | CssSyntaxKind::CSS_BOGUS_BLOCK
            | CssSyntaxKind::CSS_BOGUS_DECLARATION_ITEM
            | CssSyntaxKind::CSS_BOGUS_PROPERTY
            | CssSyntaxKind::CSS_BOGUS_PROPERTY_VALUE
            | CssSyntaxKind::CSS_BOGUS_SELECTOR
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_lf() {
        assert_eq!(detect_newline(".a {\n  color: red;\n}\n"), Newline::Lf);
    }

    #[test]
    fn detects_crlf() {
        assert_eq!(
            detect_newline(".a {\r\n  color: red;\r\n}\r\n"),
            Newline::CrLf
        );
    }

    #[test]
    fn defaults_to_lf_when_there_are_no_newlines() {
        assert_eq!(detect_newline(".a{color:red}"), Newline::Lf);
    }

    #[test]
    fn a_lone_cr_later_in_the_file_does_not_make_it_crlf() {
        assert_eq!(detect_newline(".a {\n  content: \"\r\n\";\n}"), Newline::Lf);
    }

    #[test]
    fn detects_two_space_indent() {
        assert_eq!(detect_indent(".a {\n  color: red;\n}\n"), "  ");
    }

    #[test]
    fn detects_four_space_indent() {
        assert_eq!(detect_indent(".a {\n    color: red;\n}\n"), "    ");
    }

    #[test]
    fn detects_tab_indent() {
        assert_eq!(detect_indent(".a {\n\tcolor: red;\n}\n"), "\t");
    }

    #[test]
    fn falls_back_to_two_spaces_when_nothing_is_indented() {
        assert_eq!(detect_indent(".a{color:red}"), "  ");
        assert_eq!(detect_indent(""), "  ");
    }

    #[test]
    fn nested_indentation_still_reports_one_level() {
        let src = "@media (min-width: 1px) {\n  .a {\n    color: red;\n  }\n}\n";
        assert_eq!(detect_indent(src), "  ");
    }

    #[test]
    fn balanced_braces_are_recognised() {
        assert!(ParseCtx::parse_default(".a { color: red; }\n").braces_are_balanced());
        assert!(ParseCtx::parse_default("").braces_are_balanced());
        assert!(ParseCtx::parse_default("@media print { .a { b: c; } }").braces_are_balanced());
    }

    #[test]
    fn unbalanced_braces_are_rejected() {
        assert!(!ParseCtx::parse_default(".a {\n  color: red;\n").braces_are_balanced());
        assert!(!ParseCtx::parse_default(".a {}\n}\n").braces_are_balanced());
        assert!(!ParseCtx::parse_default("}").braces_are_balanced());
    }

    #[test]
    fn braces_inside_strings_and_comments_do_not_count() {
        assert!(ParseCtx::parse_default(".a::after { content: \"{\"; }").braces_are_balanced());
        assert!(ParseCtx::parse_default("/* { */ .a { b: c; }").braces_are_balanced());
    }

    #[test]
    fn ctx_reports_file_shape() {
        let ctx = ParseCtx::parse_default(".a {\n  color: red;\n}\n");
        assert_eq!(ctx.nl(), "\n");
        assert_eq!(ctx.indent(), "  ");
        assert!(ctx.has_final_newline());
        assert!(!ctx.has_bom());
        assert!(ctx.round_trips());
    }

    #[test]
    fn ctx_detects_missing_final_newline_and_bom() {
        let ctx = ParseCtx::parse_default("\u{feff}.a { color: red; }");
        assert!(!ctx.has_final_newline());
        assert!(ctx.has_bom());
        assert!(ctx.round_trips());
    }

    #[test]
    fn the_bom_is_stripped_from_the_parsed_source_and_restored_on_output() {
        let ctx = ParseCtx::parse_default("\u{feff}.a { color: red; }\n");
        assert_eq!(ctx.source(), ".a { color: red; }\n");
        assert_eq!(
            ctx.restore_bom(ctx.source().to_string()),
            "\u{feff}.a { color: red; }\n"
        );
    }

    #[test]
    fn restore_bom_is_a_no_op_without_one() {
        let ctx = ParseCtx::parse_default(".a { color: red; }\n");
        assert_eq!(ctx.restore_bom("x".into()), "x");
    }

    #[test]
    fn indent_at_returns_the_line_prefix() {
        let src = ".a {\n    color: red;\n}\n";
        let ctx = ParseCtx::parse_default(src);
        let at = src.find("color").unwrap();
        assert_eq!(ctx.indent_at(at), "    ");
        assert!(ctx.is_at_line_start(at - 4));
    }

    #[test]
    fn line_helpers_agree_on_boundaries() {
        let src = "ab\ncd\n";
        let ctx = ParseCtx::parse_default(src);
        assert_eq!(ctx.line_start(4), 3);
        assert_eq!(ctx.line_end_inclusive(3), 6);
        assert_eq!(ctx.line_end_inclusive(0), 3);
    }

    #[test]
    fn line_comments_are_tolerated_by_default() {
        let ctx = ParseCtx::parse_default("// hi\n.a { color: red; }\n");
        assert!(ctx.round_trips());
        assert!(
            !ctx.has_errors(),
            "`//` should parse as a comment by default"
        );
    }

    #[test]
    fn strict_mode_rejects_line_comments_but_still_round_trips() {
        let ctx = ParseCtx::new("// hi\n.a { color: red; }\n", ParseOptions::strict());
        assert!(ctx.round_trips());
        assert!(ctx.has_errors());
    }
}
