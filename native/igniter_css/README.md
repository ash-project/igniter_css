<!--
SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>

SPDX-License-Identifier: MIT
-->

# NIF for Elixir.IgniterCss.Native

CSS codemods over Biome's lossless CSS CST.

## Architecture

Parse losslessly → locate byte ranges → splice text. **The tree is never
reprinted.** That is the single most important decision in this crate: it is why
comments, indentation and property order outside an edit are preserved by
construction rather than by effort.

```
source (String)
  → parse_css()                 // lossless CST, error tolerant
  → locate target nodes         // typed queries
  → node.text_trimmed_range()   // exact byte offsets
  → Vec<Edit>                   // { start, end, replacement }
  → splice into the ORIGINAL source
  → new source
```

| Module | Responsibility |
|---|---|
| `ctx` | source, parse, newline style, indent unit, BOM, brace balance |
| `locate` | typed CST queries returning byte ranges |
| `trivia` | which comments a deleted node owns (rule D) |
| `edit` | overlap-checked splicing |
| `ops/` | the codemods — diff-minimal and idempotent |
| `analyze` | read-only queries |
| `transform` | whole-file minify/beautify/merge — **not** codemods |
| `nif` | the Elixir boundary |

`ctx` and `locate` are the only modules that name Biome types. Keeping them
contained means a Biome upgrade touches two files rather than twenty.

## Building

The NIF builds along with the Elixir project. To force a local build instead of
downloading a precompiled artifact:

```
IGNITERCSS_BUILD=1 mix compile
```

## Testing

```
cargo test              # unit, corpus-invariant and property suites
cargo clippy --all-targets
cargo fmt --check
```

`tests/phase0_roundtrip.rs` is the gate everything else rests on:
`parse.syntax().to_string() == source` must hold byte-for-byte across the whole
fixture corpus in `test/fixtures`. If it ever fails, byte-range editing is no
longer safe and the codemods must not run.

## Dependency pinning

The `biome_*` crates are Biome-internal, published at 0.5.x with no API
stability guarantee, and they churn between patch releases. They are pinned with
`=` on purpose. Upgrading is a deliberate, tested activity — never a
`cargo update` — and any API call must be checked against
<https://docs.rs/biome_css_syntax> for the pinned version rather than written
from memory.
