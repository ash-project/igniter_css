<!--
SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>

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

Semantic patches for CSS files that a user owns, for
[Igniter](https://hexdocs.pm/igniter). Powered by a Rust parser (Biome's
lossless CSS CST) integrated via NIFs.

This is a **codemod** tool, not a formatter, minifier or bundler. It exists to
change the two lines you meant to change in somebody's `app.css` and nothing
else.

## Installation

```elixir
{:igniter_css, "~> 1.0.0", only: [:dev, :test]}
```

Precompiled NIFs ship for the standard target matrix, so no Rust toolchain is
needed. Set `IGNITERCSS_BUILD=1` to force a local build.

## Guarantees

1. **Comments are never lost.** Not mostly preserved — never lost.
2. **Diffs are minimal.** `git diff` shows only the lines the codemod meant to
   change. No whole-file reformatting, ever.
3. **Everything is idempotent.** Installers get re-run; the second run reports
   `changed: false` and produces identical bytes.
4. **Input is never destroyed.** A file that cannot be understood well enough to
   patch safely comes back untouched with `{:error, reason}`.

These are not aspirations. Guarantees 1–4 are asserted for every operation
against every fixture in the test corpus — a real Phoenix `app.css`, Tailwind v4
syntax, comments in awkward places, CRLF, a BOM, no trailing newline, minified
vendor CSS, non-ASCII content, and files that are simply broken — plus property
tests over generated and deliberately malformed input.

The mechanism is what makes them cheap: the parse is lossless, operations locate
**byte ranges** and splice text into the original string, and the tree is never
reprinted. Text outside an edit cannot change because nothing ever rewrites it.

## Usage

```elixir
css = """
@import "tailwindcss";
@source "../js";

.btn {
  color: red; /* brand */
}
"""

{:ok, out} = IgniterCss.ensure_at_rule(css, ~s|@plugin "daisyui";|)
{:ok, out} = IgniterCss.set_declaration(out.source, ".btn", "color", "var(--brand)")

out.source
# @import "tailwindcss";
# @source "../js";
# @plugin "daisyui";
#
# .btn {
#   color: var(--brand); /* brand */
# }
```

The `@plugin` line lands after the at-rule prologue rather than at the top of
the file, the inline comment survives, and running the same two calls again
changes nothing.

### Inside an Igniter installer

```elixir
def install(igniter, _opts) do
  path = "assets/css/app.css"

  igniter
  |> IgniterCss.Codemods.ensure_at_rule(path, ~s|@plugin "daisyui";|)
  |> IgniterCss.Codemods.ensure_rule(path, ".hide-scrollbar")
  |> IgniterCss.Codemods.set_declaration(path, ".hide-scrollbar", "scrollbar-width", "none")
end
```

Callers get Igniter's normal diff preview and confirmation flow.

## Operations

**Codemods** (`IgniterCss`) — diff-minimal, idempotent:

| | |
|---|---|
| `ensure_at_rule/3`, `remove_at_rule/4` | `@import`, `@plugin`, `@source`, `@layer`, … |
| `add_import/4`, `remove_import/3` | `@import` convenience wrappers |
| `ensure_rule/4`, `remove_rule/3` | whole rules |
| `replace_rule_body/4`, `append_raw_to_rule/4` | rule bodies |
| `set_declaration/5`, `remove_declaration/4` | declarations |
| `add_vendor_prefixes/4` | prefixed copies of a property |
| `sort_properties/2`, `remove_duplicates/2` | tidying, by moving and deleting whole lines |

**Queries** — read-only: `has_rule?/3`, `has_declaration?/4`, `has_at_rule?/3`,
`get_declaration/4`, `get_rule_declarations/3`, `get_at_rules/4`,
`list_selectors/2`, `analyze/2`,
`validate/2`, `extract_colors/2`, `extract_media_queries/2`,
`extract_animations/2`.

**Whole-file transforms** (`IgniterCss.Transform`) — `minify/2`, `beautify/2`,
`merge_stylesheets/2`. These rewrite every byte by design and are kept out of
the codemod API deliberately. Do not use them to patch a file a user maintains.

`IgniterCss.Parsers.Parser` offers the same functionality on the
`{:ok, :function_name, result}` convention shared with `igniter_js`, and accepts
a file path as well as content.

## Selector matching

Matching is strict, because guessing is how a codemod produces a surprising
diff:

- **top-level rules only** — `.b` inside `@media print` is not matched;
- selectors compare on a normalised form (`.a>.b` matches `.a > .b`), never on
  raw equality and never on a substring or fuzzy basis;
- a selector list matches as a whole — `.a` does not match `.a, .b`;
- **more than one match is an error**, not an arbitrary choice.

## Comment ownership on delete

When a codemod removes a declaration or a rule:

```css
/* ===== Layout ===== */   <- KEPT (reads as a section header)

/* used by the sidebar */  <- KEPT (blank line separates it from the target)

/* brand color */          <- DELETED (adjacent, on its own line)
color: red; /* legacy */   <- DELETED (the target and its trailing comment)
```

A section header is a comment spanning several lines, or one containing a rule
of three or more repeated `= - * # ~ _` characters.

## Tailwind v4

`@import`, `@plugin` (with and without a block), `@source`, `@theme`,
`@custom-variant`, `@variant`, `@utility`, `@apply`, `@layer` and `@reference`
all parse cleanly and are covered by the fixture corpus. Where a construct is
not in the grammar, Biome's error tolerance turns it into a node that still
carries its original text, so patching around it stays safe.

## Development

```
mix test                                    # Elixir suite
cd native/igniter_css && cargo test         # Rust suite
mix check                                   # format, credo, dialyzer, reuse
```

### Releasing

The cross-compile matrix in CI attaches a NIF per target to the GitHub release.
Once those artifacts exist, generate the checksum file — the package will not
work without it — and verify the tarball before publishing:

```
mix rustler_precompiled.download IgniterCss.Native --all --print
mix hex.build --unpack
```

`checksum-Elixir.IgniterCss.Native.exs` is listed in `files:` in `mix.exs` and
is deliberately not committed: it is only meaningful once the release artifacts
it hashes exist.
