# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.CssCase do
  @moduledoc """
  Shared assertions for the CSS test suites.

  The interesting ones are `assert_idempotent/2`, `assert_comments_preserved/2`
  and `assert_changed_lines/3` — the three properties that, together, are what
  "diff-minimal codemod" actually means.
  """

  use ExUnit.CaseTemplate

  using do
    quote do
      import IgniterCss.CssCase
    end
  end

  @fixture_dir Path.expand("../fixtures", __DIR__)

  @doc "Read a fixture from `test/fixtures`."
  def fixture(name), do: File.read!(Path.join(@fixture_dir, name))

  @doc "Every fixture as `{name, contents}`, sorted."
  def fixtures do
    @fixture_dir
    |> File.ls!()
    |> Enum.filter(&String.ends_with?(&1, ".css"))
    |> Enum.sort()
    |> Enum.map(&{&1, fixture(&1)})
  end

  @doc """
  Assert that applying `fun` twice equals applying it once, and that the second
  run reports `changed: false`.
  """
  def assert_idempotent(source, fun) do
    {:ok, once} = fun.(source)
    {:ok, twice} = fun.(once.source)

    ExUnit.Assertions.assert(
      once.source == twice.source,
      "operation is not idempotent\n\nfirst:\n#{once.source}\nsecond:\n#{twice.source}"
    )

    ExUnit.Assertions.refute(
      twice.changed,
      "operation reported changed: true on the second run"
    )

    once
  end

  @doc "Comment texts found in a stylesheet, sorted."
  def comments(source) do
    ~r{/\*.*?\*/|//[^\n]*}s
    |> Regex.scan(strip_strings(source))
    |> List.flatten()
    |> Enum.sort()
  end

  # Blank out string literals so `content: "/* not a comment */"` is not
  # counted, which would make the assertion below lie in both directions.
  defp strip_strings(source) do
    Regex.replace(~r/"(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'/s, source, ~s|""|)
  end

  @doc "Assert every comment in `before` still appears in `after_source`."
  def assert_comments_preserved(before, after_source) do
    missing = comments(before) -- comments(after_source)

    ExUnit.Assertions.assert(
      missing == [],
      "lost #{length(missing)} comment(s): #{inspect(missing)}"
    )
  end

  @doc "Added plus removed lines between two versions, via an LCS diff."
  def changed_lines(before, after_source) do
    a = String.split(before, "\n")
    b = String.split(after_source, "\n")
    common = lcs_length(a, b)
    length(a) - common + (length(b) - common)
  end

  @doc "Assert a codemod changed no more than `budget` lines."
  def assert_changed_lines(before, after_source, budget) do
    actual = changed_lines(before, after_source)

    ExUnit.Assertions.assert(
      actual <= budget,
      "changed #{actual} lines, budget was #{budget}\n\nbefore:\n#{before}\nafter:\n#{after_source}"
    )
  end

  # Standard LCS, right to left, one row at a time. Tuples rather than lists so
  # the inner reads are O(1) -- the fixtures are small but this runs per op.
  defp lcs_length(a, b) do
    b_tuple = List.to_tuple(b)
    width = tuple_size(b_tuple)
    empty_row = Tuple.duplicate(0, width + 1)

    a
    |> Enum.reverse()
    |> Enum.reduce(empty_row, fn a_item, next_row ->
      Enum.reduce(row_indices(width), next_row, fn j, current_row ->
        value =
          if a_item == elem(b_tuple, j) do
            elem(next_row, j + 1) + 1
          else
            max(elem(next_row, j), elem(current_row, j + 1))
          end

        put_elem(current_row, j, value)
      end)
    end)
    |> elem(0)
  end

  defp row_indices(0), do: []
  defp row_indices(width), do: (width - 1)..0//-1
end
