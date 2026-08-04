// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

//! The codemods. Every op in here obeys these shared rules:
//!
//! * **Idempotent** -- check then edit; if the desired state already holds,
//!   produce zero edits and report `changed: false`.
//! * **Indentation** -- inserted lines copy the indentation of the sibling they
//!   land next to; empty bodies use the file's inferred indent unit.
//! * **Newlines** -- always `ctx.nl()`, never a hardcoded `\n`.
//! * **Comment ownership on delete** -- see [`crate::trivia`].
//! * **Value-only replacement** -- changing a value edits the value range
//!   alone, so inline comments and `!important` survive.

pub mod at_rule;
pub mod declaration;
pub mod rule;
pub mod tidy;

use crate::ctx::{ParseCtx, ParseOptions};
use crate::edit::{apply_edits, prune_noop_edits, Edit};
use crate::error::{CssError, Result};

/// What every mutating op returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub source: String,
    pub changed: bool,
    pub diagnostics: Vec<String>,
}

impl Outcome {
    pub fn unchanged(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            changed: false,
            diagnostics: Vec::new(),
        }
    }

    pub fn with_diagnostic(mut self, msg: impl Into<String>) -> Self {
        self.diagnostics.push(msg.into());
        self
    }
}

/// Run a codemod end to end.
///
/// The round-trip assertion here is what keeps every edit honest: if the parse
/// does not reproduce the source byte for byte we cannot trust any offset it
/// gives us, so we refuse to patch and leave the file untouched rather than
/// risk a wrong edit.
pub fn run<F>(source: &str, options: ParseOptions, build: F) -> Result<Outcome>
where
    F: FnOnce(&ParseCtx) -> Result<Vec<Edit>>,
{
    crate::ctx::check_nesting(source)?;
    let ctx = ParseCtx::try_new(source, options)?;
    if !ctx.round_trips() {
        return Err(CssError::Unparseable(
            "the parser did not reproduce the input byte for byte".to_string(),
        ));
    }
    // Offsets in an unbalanced file do not mean what they appear to mean: a
    // "top-level" insertion would land inside an unterminated block. Refuse.
    if !ctx.braces_are_balanced() {
        return Err(CssError::Unparseable(
            "braces are unbalanced; refusing to patch".to_string(),
        ));
    }

    let edits = prune_noop_edits(ctx.source(), build(&ctx)?);
    if edits.is_empty() {
        return Ok(Outcome::unchanged(source));
    }

    let patched = apply_edits(ctx.source(), edits)?;
    let patched = ctx.restore_bom(patched);
    let changed = patched != source;
    Ok(Outcome {
        source: patched,
        changed,
        diagnostics: Vec::new(),
    })
}

/// Read-only equivalent of [`run`], for the query ops.
pub fn query<T, F>(source: &str, options: ParseOptions, f: F) -> Result<T>
where
    F: FnOnce(&ParseCtx) -> Result<T>,
{
    crate::ctx::check_nesting(source)?;
    let ctx = ParseCtx::try_new(source, options)?;
    if !ctx.round_trips() {
        return Err(CssError::Unparseable(
            "the parser did not reproduce the input byte for byte".to_string(),
        ));
    }
    f(&ctx)
}

// ---------------------------------------------------------------------------
// Shared insertion helpers
// ---------------------------------------------------------------------------

/// Re-indent a caller-supplied block of text to `indent`, preserving its own
/// relative nesting. Blank lines stay blank rather than becoming trailing
/// whitespace.
pub fn reindent(text: &str, indent: &str, nl: &str) -> String {
    let lines: Vec<&str> = text.trim_matches(['\r', '\n']).split('\n').collect();

    // The smallest indentation across non-blank lines is the block's own base.
    let base = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|line| {
            let line = line.trim_end_matches('\r');
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{indent}{}", &line[base.min(line.len())..])
            }
        })
        .collect::<Vec<_>>()
        .join(nl)
}

/// Split caller-supplied declaration text (`"color: red; margin: 0"`) into
/// individual `"color: red;"` statements.
///
/// Parsed with Biome rather than scanned: the text is wrapped in a throwaway
/// rule and the declarations are read back off the CST. That is why a `;`
/// inside `url(...)`, inside a string, or inside a comment does not split --
/// not because of bracket counting, but because the parser knows what those
/// are. Falls back to the raw text as a single declaration if it will not
/// parse, so a caller always gets something rather than silent loss.
pub fn split_declarations(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let probe = format!("a{{{text}}}");
    let Ok(ctx) = ParseCtx::try_new(&probe, ParseOptions::default()) else {
        return vec![ensure_semicolon(text.trim())];
    };
    let Some(rule) = crate::locate::find_top_level_rules(&ctx).into_iter().next() else {
        return vec![ensure_semicolon(text.trim())];
    };

    let declarations = crate::locate::declarations_in(&ctx, &rule);
    if declarations.is_empty() {
        return vec![ensure_semicolon(text.trim())];
    }

    declarations
        .into_iter()
        .map(|d| ensure_semicolon(ctx.source()[d.start..d.end].trim()))
        .collect()
}

fn ensure_semicolon(text: &str) -> String {
    if text.ends_with(';') {
        text.to_string()
    } else {
        format!("{text};")
    }
}

/// Does this text already end in a `;` outside of strings and comments?
pub fn ends_with_semicolon(text: &str) -> bool {
    text.trim_end().ends_with(';')
}

/// Reject caller text that would make the file unparseable if spliced in.
///
/// Cheap structural guard, not a full validation: we re-parse the result
/// anyway, but catching an unbalanced brace here gives a far better error.
pub fn validate_snippet(text: &str, what: &str) -> Result<()> {
    // Text spliced into a file has to satisfy the reader's nesting limit too,
    // or we would write something we then refuse to parse.
    crate::ctx::check_nesting(text)
        .map_err(|_| CssError::InvalidInput(format!("{what} is nested too deeply")))?;

    let mut depth = 0i32;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' => {
                let quote = c;
                let mut escaped = false;
                let mut closed = false;
                for q in chars.by_ref() {
                    if escaped {
                        escaped = false;
                    } else if q == '\\' {
                        escaped = true;
                    } else if q == quote {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return Err(CssError::InvalidInput(format!(
                        "{what} has an unterminated string"
                    )));
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                let mut closed = false;
                for q in chars.by_ref() {
                    if prev == '*' && q == '/' {
                        closed = true;
                        break;
                    }
                    prev = q;
                }
                if !closed {
                    return Err(CssError::InvalidInput(format!(
                        "{what} has an unterminated comment"
                    )));
                }
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return Err(CssError::InvalidInput(format!(
                        "{what} has an unbalanced closing brace"
                    )));
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(CssError::InvalidInput(format!(
            "{what} has an unbalanced opening brace"
        )));
    }
    Ok(())
}

/// Extend `offset` past a same-line trailing comment, so an insertion lands
/// after `color: red; /* note */` rather than between them.
pub fn past_trailing_comment(ctx: &ParseCtx, comments: &[(usize, usize)], offset: usize) -> usize {
    let src = ctx.source();
    let mut e = offset;
    loop {
        let mut probe = e;
        while matches!(src.as_bytes().get(probe), Some(b' ') | Some(b'\t')) {
            probe += 1;
        }
        match comments.iter().copied().find(|(s, _)| *s == probe) {
            Some((_, c_end)) if !src[e..probe].contains('\n') => e = c_end,
            _ => return e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reindent_moves_a_block_to_a_new_base() {
        let out = reindent("color: red;\nmargin: 0;", "  ", "\n");
        assert_eq!(out, "  color: red;\n  margin: 0;");
    }

    #[test]
    fn reindent_preserves_relative_nesting() {
        let out = reindent("a {\n  b: c;\n}", "    ", "\n");
        assert_eq!(out, "    a {\n      b: c;\n    }");
    }

    #[test]
    fn reindent_strips_a_common_base_first() {
        let out = reindent("        a: 1;\n        b: 2;", "  ", "\n");
        assert_eq!(out, "  a: 1;\n  b: 2;");
    }

    #[test]
    fn reindent_leaves_blank_lines_empty() {
        let out = reindent("a: 1;\n\nb: 2;", "  ", "\n");
        assert_eq!(out, "  a: 1;\n\n  b: 2;");
    }

    #[test]
    fn reindent_uses_the_requested_newline() {
        let out = reindent("a: 1;\nb: 2;", "  ", "\r\n");
        assert_eq!(out, "  a: 1;\r\n  b: 2;");
    }

    #[test]
    fn splits_simple_declarations() {
        assert_eq!(
            split_declarations("color: red; margin: 0"),
            vec!["color: red;", "margin: 0;"]
        );
    }

    #[test]
    fn does_not_split_inside_a_url() {
        assert_eq!(
            split_declarations("background: url(data:image/svg+xml;base64,AA==)"),
            vec!["background: url(data:image/svg+xml;base64,AA==);"]
        );
    }

    #[test]
    fn does_not_split_inside_a_string() {
        assert_eq!(
            split_declarations(r#"content: "a;b"; color: red"#),
            vec![r#"content: "a;b";"#, "color: red;"]
        );
    }

    #[test]
    fn does_not_split_inside_a_comment() {
        assert_eq!(
            split_declarations("color: red /* a;b */; margin: 0"),
            vec!["color: red /* a;b */;", "margin: 0;"]
        );
    }

    #[test]
    fn ignores_empty_statements() {
        assert_eq!(split_declarations(";;color: red;;"), vec!["color: red;"]);
        assert!(split_declarations("   ").is_empty());
    }

    #[test]
    fn validate_snippet_accepts_balanced_text() {
        assert!(validate_snippet("color: red;", "value").is_ok());
        assert!(validate_snippet("a { b: c; }", "block").is_ok());
        assert!(validate_snippet(r#"content: "}"#.to_owned().as_str(), "v").is_err());
    }

    #[test]
    fn validate_snippet_rejects_unbalanced_braces() {
        assert!(validate_snippet("a { b: c;", "block").is_err());
        assert!(validate_snippet("a } b", "block").is_err());
    }

    #[test]
    fn validate_snippet_ignores_braces_inside_strings_and_comments() {
        assert!(validate_snippet(r#"content: "{";"#, "v").is_ok());
        assert!(validate_snippet("/* { */ color: red;", "v").is_ok());
    }

    #[test]
    fn validate_snippet_rejects_unterminated_comment() {
        assert!(validate_snippet("color: red; /* oops", "v").is_err());
    }
}
