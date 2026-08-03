# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.Transform do
  @moduledoc """
  Whole-file transforms. **These are not codemods.**

  Everything in `IgniterCss` is diff-minimal by construction. The functions here
  deliberately are not: minifying and beautifying rewrite every byte, and
  minifying discards comments, because that is what minifying *is*.

  Use them for build-time output and reporting. Do not use them to patch a
  user's stylesheet — `IgniterCss.set_declaration/5` and friends exist for that,
  and they never route their output through here.
  """

  alias IgniterCss.{Native, ParseOpts}

  @type opts :: keyword()

  @doc """
  Strip comments and collapse whitespace.

  Driven by the token stream rather than by text matching, so a `;` inside
  `url(...)` or a `/*` inside a string is never mistaken for syntax. Whitespace
  is kept only where removing it would change how the result tokenises, so
  `@media screen and (min-width: 40em)` keeps the space after `and`.

      iex> IgniterCss.Transform.minify(".a {\\n  color: red;\\n}\\n")
      {:ok, ".a{color:red}"}
  """
  @spec minify(String.t(), opts()) :: {:ok, String.t()} | {:error, String.t()}
  def minify(source, opts \\ []) do
    Native.minify_nif(source, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Re-print the stylesheet with one declaration per line and consistent
  indentation, keeping every comment.

      iex> IgniterCss.Transform.beautify(".a{color:red;margin:0}")
      {:ok, ".a {\\n  color: red;\\n  margin: 0;\\n}\\n"}
  """
  @spec beautify(String.t(), opts()) :: {:ok, String.t()} | {:error, String.t()}
  def beautify(source, opts \\ []) do
    Native.beautify_nif(source, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Concatenate stylesheets, then drop rules a later copy makes redundant.

  A rule is dropped only when a later one has the same selector **and** a
  byte-identical body, so an intentional override survives.

      iex> IgniterCss.Transform.merge_stylesheets([".a { color: red; }", ".a { color: red; }"])
      {:ok, ".a { color: red; }\\n"}
  """
  @spec merge_stylesheets([String.t()], opts()) :: {:ok, String.t()} | {:error, String.t()}
  def merge_stylesheets(sources, opts \\ []) when is_list(sources) do
    Native.merge_stylesheets_nif(sources, ParseOpts.new(opts)) |> unwrap()
  end

  defp unwrap({:ok, _fun, value}), do: {:ok, value}
  defp unwrap({:error, _fun, reason}), do: {:error, reason}
end
