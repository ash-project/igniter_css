# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.Parsers.Formatter do
  @moduledoc """
  CSS formatting.

  If `igniter_js` is present in the running application, its Biome-backed CSS
  formatter is used, since that is a full formatter with style options.
  Otherwise this falls back to `IgniterCss.Transform.beautify/2`, a simpler
  pretty-printer that keeps every comment and needs no extra dependency.

  The choice is made at **run time**, so adding `igniter_js` to your project is
  enough — `igniter_css` does not need recompiling, and it does not declare
  `igniter_js` as a dependency (which would pin your `rustler` version).

  Formatting rewrites the whole file. It is never used by the codemods in
  `IgniterCss` — see the note in `IgniterCss.Transform`.
  """

  alias IgniterCss.Transform

  # Built at run time rather than written as a literal: `igniter_js` is not a
  # declared dependency (declaring it would pin the consumer's `rustler`
  # version), so a literal reference would raise an "undefined module" warning
  # at compile time even though the call is properly guarded.
  defp igniter_js_formatter, do: Module.concat([:IgniterJs, :Parsers, :CSS, :Formatter])

  @doc """
  Format a stylesheet.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Formatter.format(".a{color:red}")
      iex> css
      ".a {\\n  color: red;\\n}\\n"
  """
  @spec format(String.t(), :content | :path) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def format(file_path_or_content, type \\ :content) do
    if igniter_js_available?() do
      formatter = igniter_js_formatter()
      formatter.format(file_path_or_content, type)
    else
      IgniterCss.Helpers.call_nif_fn(
        file_path_or_content,
        {:format, 2},
        fn content ->
          case Transform.beautify(content) do
            {:ok, formatted} -> {:ok, :format, formatted}
            {:error, reason} -> {:error, :format, reason}
          end
        end,
        type
      )
    end
  end

  @doc """
  Is this stylesheet already formatted?

  ## Examples

      iex> IgniterCss.Parsers.Formatter.is_formatted(".a {\\n  color: red;\\n}\\n")
      {:ok, :is_formatted, true}
  """
  @spec is_formatted(String.t(), :content | :path) ::
          {:ok, atom(), boolean()} | {:error, atom(), boolean() | String.t()}
  def is_formatted(file_path_or_content, type \\ :content) do
    if igniter_js_available?() do
      formatter = igniter_js_formatter()
      formatter.is_formatted(file_path_or_content, type)
    else
      IgniterCss.Helpers.call_nif_fn(
        file_path_or_content,
        {:is_formatted, 2},
        fn content ->
          case Transform.beautify(content) do
            {:ok, ^content} -> {:ok, :is_formatted, true}
            {:ok, _other} -> {:error, :is_formatted, false}
            {:error, reason} -> {:error, :is_formatted, reason}
          end
        end,
        type
      )
    end
  end

  defp igniter_js_available?, do: Code.ensure_loaded?(igniter_js_formatter())
end
