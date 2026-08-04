# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.ImportsTest do
  @moduledoc """
  Exhaustive coverage of at-rule insertion, de-duplication, ordering and
  removal — the operations an Igniter installer leans on hardest, and the ones
  where a duplicate or a misplaced `@import` silently breaks a user's build.

  De-duplication is decided on the at-rule's **subject**, read from the CST as a
  string-literal or `url()` token, never scanned out of the text. That is why
  every quoting and wrapping variant below collapses to the same import.
  """

  use IgniterCss.CssCase, async: true

  alias IgniterCss.Outcome

  describe "duplicate detection: the same target written differently" do
    # Each of these denotes the same import as `@import "a.css";`.
    @same_target [
      ~s|@import "a.css";|,
      ~s|@import 'a.css';|,
      ~s|@import url("a.css");|,
      ~s|@import url('a.css');|,
      ~s|@import url(a.css);|,
      ~s|@import "a.css" screen;|,
      ~s|@import "a.css" layer(base);|,
      ~s|@import "a.css" supports(display: grid);|,
      ~s|@import "a.css" screen and (min-width: 40em);|,
      ~s|@import    "a.css"   ;|
    ]

    test "no variant is added to a file that already has any other variant" do
      for existing <- @same_target, candidate <- @same_target do
        source = existing <> "\n"

        assert {:ok, %Outcome{changed: false, source: ^source}} =
                 IgniterCss.ensure_at_rule(source, candidate),
               "adding #{candidate} to #{inspect(source)} created a duplicate"
      end
    end

    test "has_at_rule? agrees with ensure_at_rule for every variant" do
      for existing <- @same_target, candidate <- @same_target do
        assert {:ok, true} = IgniterCss.has_at_rule?(existing <> "\n", candidate)
      end
    end

    test "add_import/4 sees them all as the same import too" do
      for existing <- @same_target do
        assert {:ok, %Outcome{changed: false}} =
                 IgniterCss.add_import(existing <> "\n", "a.css"),
               "add_import duplicated against #{existing}"
      end
    end

    test "remove_import/3 finds them all" do
      for existing <- @same_target do
        assert {:ok, %Outcome{changed: true, source: ""}} =
                 IgniterCss.remove_import(existing <> "\n", "a.css"),
               "remove_import did not match #{existing}"
      end
    end

    test "a caller may write the needle quoted or wrapped" do
      source = ~s|@import "a.css";\n|

      for needle <- [~s|a.css|, ~s|"a.css"|, ~s|url(a.css)|, ~s|url("a.css")|] do
        assert {:ok, %Outcome{changed: true, source: ""}} =
                 IgniterCss.remove_import(source, needle),
               "needle #{needle} did not match"
      end
    end
  end

  describe "duplicate detection: targets that only look alike" do
    test "different paths are different imports" do
      for {a, b} <- [
            {"a.css", "b.css"},
            {"a.css", "./a.css"},
            {"a.css", "a.css "},
            {"dir/a.css", "a.css"},
            {"a.css", "a.CSS"}
          ] do
        source = ~s|@import "#{a}";\n|
        {:ok, out} = IgniterCss.add_import(source, String.trim(b))

        if String.trim(a) == String.trim(b) do
          refute out.changed
        else
          assert out.changed, "#{inspect(a)} and #{inspect(b)} were wrongly merged"
        end
      end
    end

    test "the same target under a different at-rule is not a duplicate" do
      source = ~s|@import "x";\n|
      assert {:ok, %Outcome{changed: true}} = IgniterCss.ensure_at_rule(source, ~s|@plugin "x";|)
    end

    test "a string inside a comment is not mistaken for a target" do
      source = ~s|/* @import "a.css"; */\n@import "b.css";\n|
      assert {:ok, %Outcome{changed: true}} = IgniterCss.add_import(source, "a.css")
    end

    test "a string inside a block is not mistaken for a target" do
      source = ~s|@plugin "real" {\n  name: "decoy";\n}\n|

      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.ensure_at_rule(source, ~s|@plugin "real";|)

      assert {:ok, %Outcome{changed: true}} =
               IgniterCss.ensure_at_rule(source, ~s|@plugin "decoy";|)
    end

    test "a target inside a nested at-rule does not count as top level" do
      source = ~s|@media print {\n  @import "a.css";\n}\n|
      assert {:ok, %Outcome{changed: true}} = IgniterCss.add_import(source, "a.css")
    end
  end

  describe "de-duplicating a file that already has duplicates" do
    test "two identical imports both match, and removal clears both" do
      source = ~s|@import "a.css";\n@import "a.css";\n.x {}\n|
      assert {:ok, %Outcome{source: ".x {}\n"}} = IgniterCss.remove_import(source, "a.css")
    end

    test "ensure_at_rule does not add a third" do
      source = ~s|@import "a.css";\n@import url(a.css);\n|

      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.ensure_at_rule(source, ~s|@import "a.css";|)
    end

    test "remove_at_rule with no filter clears every import of that name" do
      source = ~s|@import "a";\n@import "b";\n@plugin "c";\n.x {}\n|

      assert {:ok, %Outcome{source: ~s|@plugin "c";\n.x {}\n|}} =
               IgniterCss.remove_at_rule(source, "import")
    end
  end

  describe "ordering" do
    test "an import lands after the last existing import" do
      source = ~s|@import "a";\n@import "b";\n@plugin "p";\n.x {}\n|
      {:ok, out} = IgniterCss.add_import(source, "c")
      assert out.source == ~s|@import "a";\n@import "b";\n@import "c";\n@plugin "p";\n.x {}\n|
    end

    test "an import never lands after a style rule" do
      {:ok, out} = IgniterCss.add_import(".x { color: red; }\n", "a.css")
      assert out.source == ~s|@import "a.css";\n.x { color: red; }\n|
    end

    test "an import lands after @charset, never before it" do
      source = ~s|@charset "utf-8";\n.x {}\n|
      {:ok, out} = IgniterCss.add_import(source, "a.css")
      assert out.source == ~s|@charset "utf-8";\n@import "a.css";\n.x {}\n|
    end

    test "a non-prologue at-rule lands at the end of the prologue" do
      source = ~s|@import "tailwindcss";\n@source "../js";\n\n.x {}\n|
      {:ok, out} = IgniterCss.ensure_at_rule(source, ~s|@plugin "daisyui";|)

      assert out.source ==
               ~s|@import "tailwindcss";\n@source "../js";\n@plugin "daisyui";\n\n.x {}\n|
    end

    test "an import goes above a plugin even when the plugin came first" do
      source = ~s|@plugin "p";\n.x {}\n|
      {:ok, out} = IgniterCss.add_import(source, "a.css")
      assert out.source == ~s|@import "a.css";\n@plugin "p";\n.x {}\n|
    end

    test "ordering holds on a real Phoenix app.css" do
      source = fixture("phoenix_app.css")
      {:ok, out} = IgniterCss.add_import(source, "./extra.css")

      lines = String.split(out.source, "\n")
      import_at = Enum.find_index(lines, &String.starts_with?(&1, ~s|@import "./extra.css"|))
      first_rule_at = Enum.find_index(lines, &String.starts_with?(&1, "["))

      assert import_at < first_rule_at
      assert_changed_lines(source, out.source, 1)
      assert_comments_preserved(source, out.source)
    end
  end

  describe "media queries and modifiers" do
    test "a media query is carried onto the generated line" do
      {:ok, out} = IgniterCss.add_import("", "m.css", "screen and (max-width: 768px)")
      assert out.source == ~s|@import "m.css" screen and (max-width: 768px);\n|
    end

    test "adding the same target with a different media query does not duplicate" do
      source = ~s|@import "m.css" print;\n|
      assert {:ok, %Outcome{changed: false}} = IgniterCss.add_import(source, "m.css", "screen")
    end

    test "an empty or nil media query is omitted" do
      for media <- [nil, "", "   "] do
        {:ok, out} = IgniterCss.add_import("", "m.css", media)
        assert out.source == ~s|@import "m.css";\n|
      end
    end

    test "layer() and supports() modifiers survive untouched when adding a sibling" do
      source = ~s|@import "a.css" layer(base) supports(display: grid);\n|
      {:ok, out} = IgniterCss.add_import(source, "b.css")
      assert out.source == source <> ~s|@import "b.css";\n|
    end
  end

  describe "url quoting" do
    test "relative paths are quoted, absolute ones wrapped in url()" do
      for {url, expected} <- [
            {"styles.css", ~s|@import "styles.css";\n|},
            {"./a/b.css", ~s|@import "./a/b.css";\n|},
            {"../vendor/x.css", ~s|@import "../vendor/x.css";\n|},
            {"/absolute.css", ~s|@import url("/absolute.css");\n|},
            {"https://x/y.css", ~s|@import url("https://x/y.css");\n|},
            {"http://x/y.css", ~s|@import url("http://x/y.css");\n|}
          ] do
        assert {:ok, %Outcome{source: ^expected}} = IgniterCss.add_import("", url)
      end
    end

    test "a url that cannot be quoted safely is rejected" do
      for bad <- [~s|a"b|, "a\nb", "", "   "] do
        assert {:error, _} = IgniterCss.add_import("", bad)
      end
    end

    test "a non-ascii path round-trips" do
      {:ok, out} = IgniterCss.add_import("", "./styles/café.css")
      assert out.source == ~s|@import "./styles/café.css";\n|

      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.add_import(out.source, "./styles/café.css")
    end
  end

  describe "file shape is preserved" do
    test "CRLF files stay CRLF" do
      source = ~s|@import "a";\r\n.x {}\r\n|
      {:ok, out} = IgniterCss.add_import(source, "b")
      assert out.source == ~s|@import "a";\r\n@import "b";\r\n.x {}\r\n|
    end

    test "a BOM survives" do
      {:ok, out} = IgniterCss.add_import("﻿.x {}\n", "a.css")
      assert out.source == ~s|﻿@import "a.css";\n.x {}\n|
    end

    test "a missing trailing newline stays missing" do
      {:ok, out} = IgniterCss.add_import(".x {}", "a.css")
      assert out.source == ~s|@import "a.css";\n.x {}|
    end

    test "an import lands below a file header comment but above a rule's comment" do
      header = "/* App styles.\n   Two lines. */\n\n.x {}\n"
      {:ok, out} = IgniterCss.add_import(header, "a.css")
      assert out.source == ~s|/* App styles.\n   Two lines. */\n@import "a.css";\n\n.x {}\n|

      attached = "/* about .x */\n.x {}\n"
      {:ok, out2} = IgniterCss.add_import(attached, "a.css")
      assert out2.source == ~s|/* about .x */\n@import "a.css";\n.x {}\n|
    end
  end

  describe "removal keeps the file coherent" do
    test "removing an import takes the comment it owns but not a section header" do
      source = ~s|/* ===== Imports ===== */\n/* the base sheet */\n@import "a";\n@import "b";\n|
      {:ok, out} = IgniterCss.remove_import(source, "a")
      assert out.source == ~s|/* ===== Imports ===== */\n@import "b";\n|
    end

    test "removing an absent import is a no-op" do
      source = ~s|@import "b";\n|

      assert {:ok, %Outcome{changed: false, source: ^source}} =
               IgniterCss.remove_import(source, "a")
    end

    test "removal is idempotent" do
      assert_idempotent(~s|@import "a";\n@import "b";\n|, &IgniterCss.remove_import(&1, "a"))
    end

    test "removing every import leaves valid CSS" do
      source = fixture("kitchen_sink.css")
      {:ok, out} = IgniterCss.remove_at_rule(source, "import")
      {_, validation} = IgniterCss.validate(out.source)
      assert validation.round_trips

      assert {:ok, %{imports_count: 0}} = IgniterCss.analyze(out.source)
    end
  end

  describe "other at-rule families dedupe the same way" do
    test "@plugin, @source, @use and @reference" do
      for {name, target} <- [
            {"plugin", "../vendor/daisyui"},
            {"source", "../js"},
            {"reference", "../../app.css"}
          ] do
        line = ~s|@#{name} "#{target}";|
        {:ok, first} = IgniterCss.ensure_at_rule("", line)
        assert first.changed

        # Same target, single quotes: still a duplicate.
        alt = ~s|@#{name} '#{target}';|
        assert {:ok, %Outcome{changed: false}} = IgniterCss.ensure_at_rule(first.source, alt)

        # Different target: added.
        assert {:ok, %Outcome{changed: true}} =
                 IgniterCss.ensure_at_rule(first.source, ~s|@#{name} "other";|)
      end
    end

    test "@layer has no target, so it dedupes on the whole prelude" do
      source = "@layer base, components;\n"

      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.ensure_at_rule(source, "@layer base, components;")

      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.ensure_at_rule(source, "@layer base,   components;")

      assert {:ok, %Outcome{changed: true}} =
               IgniterCss.ensure_at_rule(source, "@layer utilities;")
    end

    test "a block at-rule dedupes against its non-block form" do
      source = ~s|@plugin "../vendor/daisyui" {\n  themes: false;\n}\n|

      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.ensure_at_rule(source, ~s|@plugin "../vendor/daisyui";|)
    end
  end

  describe "installer-shaped end to end" do
    test "adding a full Tailwind v4 plugin set twice yields one copy of each" do
      # Targets, not whole lines: the fixture already carries
      # `@import "tailwindcss" source(none);`, so the literal line
      # `@import "tailwindcss";` is correctly never written — dedup is decided
      # on the subject, and asserting on the subject is what proves it.
      lines = [
        {~s|@import "tailwindcss";|, ~s|"tailwindcss"|},
        {~s|@plugin "@tailwindcss/forms";|, ~s|"@tailwindcss/forms"|},
        {~s|@plugin "@tailwindcss/typography";|, ~s|"@tailwindcss/typography"|},
        {~s|@source "../js";|, ~s|"../js"|},
        {~s|@source "../../lib/my_app_web";|, ~s|"../../lib/my_app_web"|}
      ]

      install = fn source ->
        Enum.reduce(lines, source, fn {line, _target}, acc ->
          {:ok, out} = IgniterCss.ensure_at_rule(acc, line)
          out.source
        end)
      end

      once = install.(fixture("phoenix_app.css"))
      twice = install.(once)

      assert once == twice

      for {line, target} <- lines do
        assert {:ok, true} = IgniterCss.has_at_rule?(once, line)

        occurrences = length(String.split(once, target)) - 1
        assert occurrences == 1, "target #{target} appears #{occurrences} times"
      end

      assert_comments_preserved(fixture("phoenix_app.css"), once)
      {_, validation} = IgniterCss.validate(once)
      assert validation.round_trips
    end
  end
end
