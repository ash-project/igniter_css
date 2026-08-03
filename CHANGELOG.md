<!--
SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>

SPDX-License-Identifier: MIT
-->

# Changelog for IgniterCss 0.2.0

### Breaking changes:

- The Python/`tinycss2` implementation is gone, along with the `pythonx`
  dependency, `priv/python`, `plibs/` and `rebuild_wheel.sh`. Everything is now
  a precompiled Rust NIF built on Biome's lossless CSS CST — no Python, no Node,
  no external process.
- `IgniterCss.CSS.CssProcessor` has been removed. Its pipeline mixed codemods
  with whole-file rewriting, which the new design keeps deliberately separate:
  use `IgniterCss` for patching and `IgniterCss.Transform` for build-time
  output.
- `IgniterCss.Parsers.Parser` keeps its function names and its
  `{:ok, :function_name, result}` shape, but the mutating functions are now
  diff-minimal rather than reprinting the stylesheet, and selector matching is
  strict: an ambiguous selector is an error instead of an arbitrary choice.

### Features:

- New `IgniterCss` API: `ensure_at_rule/3`, `ensure_rule/4`,
  `set_declaration/5`, `remove_declaration/4`, `remove_rule/3`,
  `replace_rule_body/4`, `append_raw_to_rule/4`, `add_vendor_prefixes/4`,
  `sort_properties/2`, `remove_duplicates/2`, plus read-only queries and
  analysis.
- New `IgniterCss.Codemods` — Igniter-facing wrappers that take and return an
  `Igniter` struct, so callers get the normal diff preview and confirmation
  flow.
- New `IgniterCss.Transform` for whole-file `minify/2`, `beautify/2` and
  `merge_stylesheets/2`, held separate from the codemods.
- Tailwind v4 support: `@theme`, `@plugin` (with and without a block),
  `@source`, `@custom-variant`, `@variant`, `@utility`, `@apply`, `@reference`.

### Improvements:

- Comments are preserved by construction: operations splice byte ranges into the
  original source instead of reprinting the tree, so text outside an edit cannot
  change.
- Every operation is idempotent and reports `changed: false` on a re-run.
- Files that cannot be patched safely — an unbalanced brace, a parse that does
  not reproduce the input byte for byte — are refused rather than half-edited.
- Inserted text follows the file's own newline style, indent unit and trailing
  newline; BOM and CRLF files round-trip.
- Precompiled NIFs, so end users need no Rust toolchain.

# Changelog for IgniterCss 0.1.1

### Improvements:

- fix(css): improve CSS comment validation and update css_tools to v0.1.2

# Changelog for IgniterCss 0.1.0

### Features:

- feat(css): add CSS parser with AST transformation support
 [#1](https://github.com/ash-project/igniter_css/pull/1)
