# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.CorpusInvariantsTest do
  @moduledoc """
  The four guarantees in `IgniterCss`'s moduledoc, checked for every operation
  against every fixture rather than only the cases somebody wrote a test for.

  The fixture corpus is real-world shaped on purpose: a Phoenix `app.css`,
  Tailwind v4 syntax, comments in awkward places, CRLF, a BOM, no trailing
  newline, minified vendor CSS, non-ASCII content, and files that are simply
  broken.
  """

  use IgniterCss.CssCase, async: true

  # {label, function, is_a_removal?}
  # A function, not a module attribute: closures cannot be escaped into one.
  defp ops do
    [
      {"ensure_at_rule/import", &IgniterCss.ensure_at_rule(&1, ~s|@import "probe.css";|), false},
      {"ensure_at_rule/plugin", &IgniterCss.ensure_at_rule(&1, ~s|@plugin "probe";|), false},
      {"ensure_at_rule/source", &IgniterCss.ensure_at_rule(&1, ~s|@source "../probe";|), false},
      {"remove_at_rule/import", &IgniterCss.remove_at_rule(&1, "import"), true},
      {"remove_at_rule/plugin", &IgniterCss.remove_at_rule(&1, "plugin"), true},
      {"ensure_rule", &IgniterCss.ensure_rule(&1, ".igniter-probe"), false},
      {"remove_rule", &IgniterCss.remove_rule(&1, ".hide-scrollbar"), true},
      {"set_declaration/create",
       &IgniterCss.set_declaration(&1, ".igniter-probe", "display", "none", create_rule: true),
       false},
      {"set_declaration/existing",
       &IgniterCss.set_declaration(&1, ".page", "color", "rebeccapurple"), false},
      {"remove_declaration", &IgniterCss.remove_declaration(&1, ".page", "display"), true},
      {"append_raw_to_rule", &IgniterCss.append_raw_to_rule(&1, ".page", "outline: 1px solid;"),
       false},
      {"replace_rule_body", &IgniterCss.replace_rule_body(&1, ".sr-only", "position: fixed;"),
       false},
      {"add_vendor_prefixes",
       &IgniterCss.add_vendor_prefixes(&1, "user-select", ["-webkit-", "-moz-"]), false},
      {"add_vendor_prefixes/display",
       &IgniterCss.add_vendor_prefixes(&1, "display", ["-webkit-"]), false},
      {"sort_properties", &IgniterCss.sort_properties/1, false},
      {"remove_duplicates", &IgniterCss.remove_duplicates/1, false}
    ]
  end

  # An op that returns `{:error, _}` for a given fixture has not applied to it;
  # that is valid behaviour (a missing rule, an ambiguous selector, a file we
  # refuse to patch), and it means the fixture is skipped for that op.
  defp applicable(fun, source) do
    case fun.(source) do
      {:ok, outcome} -> {:ok, outcome}
      {:error, _reason} -> :skip
    end
  end

  test "guarantee 3: every op is idempotent on every fixture" do
    for {name, source} <- fixtures(), {label, fun, _} <- ops() do
      with {:ok, once} <- applicable(fun, source) do
        assert {:ok, twice} = fun.(once.source),
               "#{label} succeeded on #{name} but failed on its own output"

        assert once.source == twice.source, "#{label} is not idempotent on #{name}"
        refute twice.changed, "#{label} reported changed: true on a second run over #{name}"
      end
    end
  end

  test "guarantee 1: no non-removal op ever loses a comment" do
    for {name, source} <- fixtures(),
        {label, fun, removal?} <- ops(),
        not removal?,
        comments(source) != [] do
      with {:ok, out} <- applicable(fun, source) do
        missing = comments(source) -- comments(out.source)
        assert missing == [], "#{label} lost #{inspect(missing)} from #{name}"
      end
    end
  end

  test "guarantee 4: output always still round-trips, with no new parse errors" do
    for {name, source} <- fixtures(), {label, fun, _} <- ops() do
      with {:ok, out} <- applicable(fun, source) do
        {_, before} = IgniterCss.validate(source)
        {_, after_} = IgniterCss.validate(out.source)

        assert after_.round_trips, "#{label} broke the round-trip of #{name}"

        assert after_.diagnostics <= before.diagnostics,
               "#{label} added parse diagnostics to #{name}"
      end
    end
  end

  test "an unchanged outcome returns the source byte for byte" do
    for {name, source} <- fixtures(), {label, fun, _} <- ops() do
      with {:ok, %{changed: false} = out} <- applicable(fun, source) do
        assert out.source == source, "#{label} reported changed: false but altered #{name}"
      end
    end
  end

  test "every op preserves the file's newline style" do
    for {name, source} <- fixtures(),
        String.contains?(source, "\r\n"),
        {label, fun, _} <- ops() do
      with {:ok, out} <- applicable(fun, source) do
        bare_lf_before = count_bare_lf(source)
        bare_lf_after = count_bare_lf(out.source)

        assert bare_lf_before == bare_lf_after,
               "#{label} introduced a bare LF into the CRLF file #{name}"
      end
    end
  end

  test "every op preserves a BOM" do
    for {name, source} <- fixtures(), {label, fun, _} <- ops() do
      with {:ok, out} <- applicable(fun, source) do
        assert String.starts_with?(out.source, "﻿") ==
                 String.starts_with?(source, "﻿"),
               "#{label} changed the BOM state of #{name}"
      end
    end
  end

  test "guarantee 2: single-target ops change only a handful of lines" do
    budgets = [
      {"ensure_at_rule", &IgniterCss.ensure_at_rule(&1, ~s|@plugin "probe";|), 1},
      {"ensure_rule", &IgniterCss.ensure_rule(&1, ".igniter-probe"), 4},
      {"set_declaration", &IgniterCss.set_declaration(&1, ".page", "color", "rebeccapurple"), 2},
      # A declaration plus the comments Rule D says it owns.
      {"remove_declaration", &IgniterCss.remove_declaration(&1, ".page", "display"), 3}
    ]

    for {name, source} <- fixtures(), {label, fun, budget} <- budgets do
      with {:ok, %{changed: true} = out} <- applicable(fun, source) do
        actual = changed_lines(source, out.source)

        assert actual <= budget,
               "#{label} changed #{actual} lines in #{name} (budget #{budget})"
      end
    end
  end

  test "the read-only ops survive the whole corpus" do
    for {name, source} <- fixtures() do
      assert {:ok, _} = IgniterCss.analyze(source), "analyze failed on #{name}"
      assert {:ok, _} = IgniterCss.extract_colors(source), "extract_colors failed on #{name}"

      assert {:ok, _} = IgniterCss.extract_media_queries(source),
             "extract_media_queries failed on #{name}"

      assert {:ok, _} = IgniterCss.extract_animations(source),
             "extract_animations failed on #{name}"

      assert {_, %IgniterCss.Validation{}} = IgniterCss.validate(source)
      assert {:ok, _} = IgniterCss.list_selectors(source)
    end
  end

  test "malformed input is never half-edited" do
    malformed = [
      ".broken {\n  color: red;\n",
      ".a { color: red; }\n}\n.b { color: blue; }\n",
      "{{{{",
      "@media (\n",
      "}",
      ".a { color: "
    ]

    for source <- malformed, {label, fun, _} <- ops() do
      case fun.(source) do
        {:ok, %{changed: false, source: unchanged}} ->
          assert unchanged == source,
                 "#{label} altered #{inspect(source)} while reporting no change"

        {:ok, %{source: patched}} ->
          {_, validation} = IgniterCss.validate(patched)

          assert validation.round_trips,
                 "#{label} produced non-round-tripping output from #{inspect(source)}"

        {:error, _reason} ->
          # An error carries no source, so nothing could have been written.
          :ok
      end
    end
  end

  test "a fresh Phoenix app.css can be patched end to end with a minimal diff" do
    source = fixture("phoenix_app.css")

    {:ok, s1} = IgniterCss.ensure_at_rule(source, ~s|@plugin "@tailwindcss/typography";|)
    {:ok, s2} = IgniterCss.ensure_at_rule(s1.source, ~s|@source "../vendor";|)
    {:ok, s3} = IgniterCss.ensure_rule(s2.source, ".hide-scrollbar")

    {:ok, s4} =
      IgniterCss.set_declaration(s3.source, ".hide-scrollbar", "scrollbar-width", "none")

    assert s1.changed and s2.changed and s3.changed and s4.changed

    # Two at-rule lines, a blank line, and three lines of new rule.
    assert_changed_lines(source, s4.source, 6)
    assert_comments_preserved(source, s4.source)

    # Every line the file already had is still there, verbatim.
    for line <- String.split(source, "\n") do
      assert String.contains?(s4.source, line), "patching dropped #{inspect(line)}"
    end

    # Re-running the installer produces no diff at all.
    {:ok, again} = IgniterCss.ensure_at_rule(s4.source, ~s|@plugin "@tailwindcss/typography";|)
    refute again.changed
  end

  defp count_bare_lf(string) do
    total = length(String.split(string, "\n")) - 1
    crlf = length(String.split(string, "\r\n")) - 1
    total - crlf
  end
end
