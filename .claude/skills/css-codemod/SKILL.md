---
name: css-codemod
description: Add or change a CSS codemod in igniter_css (native/igniter_css). Use when editing Rust in this repo, adding an operation, touching selector or at-rule matching, or debugging a codemod that reformats too much. Covers the byte-range architecture, the Biome CST, and the required tests.
---

# CSS codemods in igniter_css

## The one rule

**Never reprint the tree.** Parse losslessly → locate byte ranges → splice text
into the original source. Text outside an edit cannot change, which is why
comments survive by construction. Any change that produces output by printing a
node is wrong.

```
parse_css() → locate nodes → node.text_trimmed_range() → Vec<Edit> → apply_edits(original)
```

`text_range()` includes leading trivia (the comment above the node).
`text_trimmed_range()` is the node's own bytes. Almost always you want trimmed.

## Decide from the CST, never by scanning text

Biome already models what you are about to hand-parse:

| need | node/token |
|---|---|
| combinator | `CSS_COMPLEX_SELECTOR` + token; descendant is `CSS_SPACE_LITERAL` |
| nested rule | `CSS_NESTED_QUALIFIED_RULE` + `CSS_RELATIVE_SELECTOR_LIST` (**not** `CSS_QUALIFIED_RULE`) |
| hex colour | `CSS_COLOR` / `CSS_COLOR_LITERAL` (validate digits: `#notahex` parses as a colour) |
| colour fn | `CSS_FUNCTION` + identifier name |
| url payload | `CSS_URL_FUNCTION` / `CSS_URL_VALUE_RAW` — structurally not an identifier |
| at-rule target | first `CSS_STRING_LITERAL` / `CSS_URL_VALUE_RAW_LITERAL` before the block |
| comments | token leading/trailing trivia |

Text scanning is acceptable in exactly two places: caller-supplied strings that
are not CSS, and the pre-parse nesting guard (which must not recurse, because
recursing is what it prevents).

## Safety

- No `unwrap`/`expect`/indexing reachable from a NIF. Rustler catches panics, but
  a **stack overflow aborts** and takes the VM down.
- Every parse goes through `ParseCtx::try_new` — Biome 0.5.8 panics on some
  input ("parser is no longer progressing").
- `check_nesting` before any parse. Limit 256; real CSS is depth ~7.
- Caller text spliced in must pass the same nesting limit, or you write a file
  you would refuse to read.

## Matching

Top-level only unless the caller opts in. Normalised comparison, never substring
or fuzzy. A selector list matches whole. **More than one match is an error** —
never pick one.

## Required per codemod

Same commit, no exceptions:

1. golden test (input + op → expected output, exact string)
2. idempotency test (twice == once, second reports `changed: false`)
3. comment-placement test (trailing, adjacent-above, blank-line-separated, section header)

Then add the op to the sweeps in `tests/corpus_invariants.rs` and
`test/corpus_invariants_test.exs`, which assert those properties for every op
against every fixture.

## Verify

```bash
cd native/igniter_css
cargo test && cargo fmt --check && cargo clippy --all-targets -- -D warnings
cd - && IGNITERCSS_BUILD=1 mix test && mix credo --strict
```

`tests/roundtrip.rs` is the gate: `parse.syntax().to_string() == source` across
the whole corpus. If it fails, byte-range editing is unsafe — stop.

## Gotchas

- BOM: stripped in `ParseCtx`, restored on output. Biome lexes U+FEFF into the
  first identifier and silently breaks selector matching otherwise.
- Unbalanced braces: refuse. "Top level" is meaningless in such a file.
- Never call `find_all_*` inside a per-node loop — that is quadratic. Build the
  ref from the node you already hold (`locate::at_rule_ref`).
