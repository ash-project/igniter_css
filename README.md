<!--
SPDX-FileCopyrightText: 2025 Shahryar Tavakkoli

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

IgniterCss is CSS patching functionality for [Igniter](https://hexdocs.pm/igniter)

## Installation

IgniterCss can be added to an existing elixir project by adding it to your dependencies:

```elixir
{:igniter_css, "~> 0.1.1", only: [:dev, :test]}
```

## Status

We are still working on getting this ready for an initial release.

The initial codemods will be limited to specific transformations. This is not intended to
be a toolkit (yet) for writing any arbitrary transformation like `Igniter` is for `Elixir`.
We will likely provide a way to do this by the user providing rust code and using our tools
to hook it up to igniter.
