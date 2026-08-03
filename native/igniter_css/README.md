<!--
SPDX-FileCopyrightText: 2025 Shahryar Tavakkoli
SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs.contributors>

SPDX-License-Identifier: MIT
-->

<img src="https://github.com/ash-project/igniter/blob/main/logos/igniter-logo-small.png?raw=true#gh-light-mode-only" alt="Logo Light" width="250">
<img src="https://github.com/ash-project/igniter/blob/main/logos/igniter-logo-small.png?raw=true#gh-dark-mode-only" alt="Logo Dark" width="250">

[![CI](https://github.com/ash-project/igniter_css/actions/workflows/elixir.yml/badge.svg)](https://github.com/ash-project/igniter_css/actions/workflows/elixir.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Hex version badge](https://img.shields.io/hexpm/v/igniter_css.svg)](https://hex.pm/packages/igniter_css)
[![Hexdocs badge](https://img.shields.io/badge/docs-hexdocs-purple)](https://hexdocs.pm/igniter_css)
[![REUSE status](https://api.reuse.software/badge/github.com/ash-project/igniter_css)](https://api.reuse.software/info/github.com/ash-project/igniter_css)

# IgniterCss

CSS codemods for [Igniter](https://hexdocs.pm/igniter), powered by a Rust parser
integrated via NIFs. Changes the lines you meant to change, and nothing else.

```elixir
{:igniter_css, "~> 0.2.0", only: [:dev, :test]}
```

Precompiled NIFs ship for the standard target matrix — no Rust toolchain needed.

- **Comments are never lost** — operations splice byte ranges, the tree is never reprinted
- **Minimal diffs** — no whole-file reformatting
- **Idempotent** — re-running reports `changed: false`
- **Safe** — a file that can't be patched cleanly comes back untouched with `{:error, _}`

## Usage

```elixir
css = """
@import "tailwindcss";

.btn {
  color: red; /* brand */
}
"""

{:ok, out} = IgniterCss.ensure_at_rule(css, ~s|@plugin "daisyui";|)
{:ok, out} = IgniterCss.set_declaration(out.source, ".btn", "color", "var(--brand)")
```

```css
@import "tailwindcss";
@plugin "daisyui";

.btn {
  color: var(--brand); /* brand */
}
```

Everything returns `{:ok, %IgniterCss.Outcome{source:, changed:, diagnostics:}}`
or `{:error, reason}`, plus an optional trailing keyword list.

In an installer, for Igniter's diff preview and confirmation:

```elixir
igniter
|> IgniterCss.Codemods.ensure_at_rule(path, ~s|@plugin "daisyui";|)
|> IgniterCss.Codemods.ensure_rule(path, ".hide-scrollbar")
|> IgniterCss.Codemods.set_declaration(path, ".hide-scrollbar", "scrollbar-width", "none")
```

## Operations

| `IgniterCss` | |
|---|---|
| `ensure_at_rule/3` · `remove_at_rule/4` | `@import`, `@plugin`, `@source`, `@layer`, … |
| `add_import/4` · `remove_import/3` | `@import` convenience |
| `ensure_rule/4` · `remove_rule/3` | whole rules |
| `replace_rule_body/4` · `append_raw_to_rule/4` | rule bodies |
| `set_declaration/5` · `remove_declaration/4` | declarations |
| `add_vendor_prefixes/4` | prefixed copies of a property |
| `sort_properties/2` · `remove_duplicates/2` | tidying, by moving whole lines |

Read-only: `has_rule?/3` · `has_declaration?/4` · `has_at_rule?/3` ·
`get_declaration/4` · `get_rule_declarations/3` · `list_selectors/2` ·
`analyze/2` · `validate/2` · `extract_colors/2` · `extract_media_queries/2` ·
`extract_animations/2`

`IgniterCss.Transform` — `minify/2`, `beautify/2`, `merge_stylesheets/2`. These
rewrite every byte by design; don't point them at a file a user maintains.

`IgniterCss.Parsers.Parser` — the same surface on the
`{:ok, :function_name, result}` convention shared with `igniter_js`, and accepts
a file path as well as content.

## Matching

Top-level rules only, compared on a normalised form (`.a>.b` matches `.a > .b`),
never substring or fuzzy. `.a` does not match `.a, .b`. **Ambiguity is an
error**, not a guess. All selector kinds are supported — class, id, tag,
attribute, pseudo, combinators, escaped and non-ASCII.

Tailwind v4 parses cleanly: `@theme`, `@plugin`, `@source`, `@custom-variant`,
`@variant`, `@utility`, `@apply`, `@layer`, `@reference`.

When a codemod deletes a node, it takes the comments that node owns:

```css
/* ===== Layout ===== */   ← kept (section header)

/* used by the sidebar */  ← kept (blank line between)

/* brand color */          ← deleted (adjacent, own line)
color: red; /* legacy */   ← deleted (target + trailing)
```

## Contributing

```bash
mix deps.get
IGNITERCSS_BUILD=1 mix compile   # build the NIF locally; required until a release exists

mix test                                   # Elixir
cd native/igniter_css && cargo test        # Rust
mix check                                  # format, credo, dialyzer, reuse
```

CI runs the Rust checks as `-D warnings`:

```bash
cd native/igniter_css
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

| module | |
|---|---|
| `ctx` | source, parse, newline style, indent unit, BOM, brace balance |
| `locate` | CST queries returning byte ranges |
| `trivia` | which comments a deleted node owns |
| `edit` | overlap-checked splicing |
| `ops/` | the codemods |
| `analyze` · `transform` | read-only queries · whole-file rewrites |
| `nif` | the Elixir boundary |

Four rules for changes:

- Never reprint the tree — locate byte ranges and splice.
- No `unwrap`/`expect` reachable from a NIF; a panic takes down a scheduler.
- Every codemod ships with a golden, an idempotency and a comment-placement test.
- `biome_*` crates are pinned with `=` and churn between patch releases — check
  [docs.rs](https://docs.rs/biome_css_syntax) rather than writing a call from memory.

`ctx` and `locate` are the only modules naming Biome types, so an upgrade touches
two files. `tests/phase0_roundtrip.rs` is the gate everything rests on:
`parse.syntax().to_string() == source`, byte for byte, across the whole corpus.
