# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.CodemodsTest do
  @moduledoc """
  The Igniter-facing wrappers, exercised against a real `Igniter` struct so the
  installer flow — including "re-running changes nothing" — is covered rather
  than assumed.
  """

  use IgniterCss.CssCase, async: true

  import Igniter.Test

  alias IgniterCss.Codemods

  @path "assets/css/app.css"

  defp igniter_with(content) do
    test_project(files: %{@path => content})
  end

  defp content(igniter) do
    igniter.rewrite
    |> Rewrite.source!(@path)
    |> Rewrite.Source.get(:content)
  end

  describe "add_import/5 and remove_import/4" do
    test "adds an import through Igniter, so the change is in the diff" do
      result =
        ~s|@import "tailwindcss";\n|
        |> igniter_with()
        |> Codemods.add_import(@path, "../vendor/app.css")
        |> content()

      assert result == ~s|@import "tailwindcss";\n@import "../vendor/app.css";\n|
    end

    test "re-adding the same import leaves the file untouched" do
      source = ~s|@import "tailwindcss";\n@import "../vendor/app.css";\n|

      result =
        source
        |> igniter_with()
        |> Codemods.add_import(@path, "../vendor/app.css")
        |> content()

      assert result == source
    end

    test "an import lands after the prologue and keeps surrounding comments" do
      result =
        ~s|/* app styles */\n@import "tailwindcss";\n\n.btn {\n  color: red;\n}\n|
        |> igniter_with()
        |> Codemods.add_import(@path, "../vendor/app.css")
        |> content()

      assert result =~ "/* app styles */"
      assert result =~ ".btn {\n  color: red;\n}"

      {tailwind, _} = :binary.match(result, "tailwindcss")
      {vendor, _} = :binary.match(result, "../vendor/app.css")
      assert tailwind < vendor
    end

    test "removing an import takes only that line" do
      result =
        ~s|@import "tailwindcss";\n@import "../vendor/app.css";\n\n.btn {\n  color: red;\n}\n|
        |> igniter_with()
        |> Codemods.remove_import(@path, "../vendor/app.css")
        |> content()

      refute result =~ "../vendor/app.css"
      assert result =~ "tailwindcss"
      assert result =~ ".btn {\n  color: red;\n}"
    end

    test "removing an import that is not there changes nothing" do
      source = ~s|@import "tailwindcss";\n|

      result =
        source
        |> igniter_with()
        |> Codemods.remove_import(@path, "../vendor/nope.css")
        |> content()

      assert result == source
    end
  end

  describe "ensure_at_rule/4" do
    test "adds the at-rule to the file" do
      result =
        ~s|@import "tailwindcss";\n|
        |> igniter_with()
        |> Codemods.ensure_at_rule(@path, ~s|@plugin "daisyui";|)
        |> content()

      assert result == ~s|@import "tailwindcss";\n@plugin "daisyui";\n|
    end

    test "re-running leaves the file untouched" do
      source = ~s|@import "tailwindcss";\n@plugin "daisyui";\n|

      result =
        source
        |> igniter_with()
        |> Codemods.ensure_at_rule(@path, ~s|@plugin "daisyui";|)
        |> content()

      assert result == source
    end
  end

  describe "ensure_rule/5 and set_declaration/6" do
    test "a full installer run produces the expected file" do
      result =
        fixture("phoenix_app.css")
        |> igniter_with()
        |> Codemods.ensure_at_rule(@path, ~s|@plugin "@tailwindcss/typography";|)
        |> Codemods.ensure_rule(@path, ".hide-scrollbar")
        |> Codemods.set_declaration(@path, ".hide-scrollbar", "scrollbar-width", "none")
        |> content()

      assert String.contains?(result, ~s|@plugin "@tailwindcss/typography";|)
      assert String.contains?(result, ".hide-scrollbar {\n  scrollbar-width: none;\n}")
      assert_comments_preserved(fixture("phoenix_app.css"), result)
      assert_changed_lines(fixture("phoenix_app.css"), result, 5)
    end

    test "running the same installer twice is a no-op" do
      install = fn igniter ->
        igniter
        |> Codemods.ensure_at_rule(@path, ~s|@plugin "daisyui";|)
        |> Codemods.ensure_rule(@path, ".hide-scrollbar")
        |> Codemods.set_declaration(@path, ".hide-scrollbar", "scrollbar-width", "none")
      end

      once = fixture("phoenix_app.css") |> igniter_with() |> install.() |> content()
      twice = once |> igniter_with() |> install.() |> content()

      assert once == twice
    end

    test "seeds a new rule with declarations" do
      result =
        ".a {}\n"
        |> igniter_with()
        |> Codemods.ensure_rule(@path, ".b", "color: red")
        |> content()

      assert result == ".a {}\n\n.b {\n  color: red;\n}\n"
    end
  end

  describe "remove_declaration/5 and remove_rule/4" do
    test "removes a declaration with the comment it owns" do
      result =
        ".a {\n  /* legacy */\n  color: red;\n  margin: 0;\n}\n"
        |> igniter_with()
        |> Codemods.remove_declaration(@path, ".a", "color")
        |> content()

      assert result == ".a {\n  margin: 0;\n}\n"
    end

    test "removes a rule but keeps a section header" do
      result =
        "/* ===== Utils ===== */\n.b {}\n.c {}\n"
        |> igniter_with()
        |> Codemods.remove_rule(@path, ".b")
        |> content()

      assert result == "/* ===== Utils ===== */\n.c {}\n"
    end
  end

  describe "append_raw_to_rule/5 and add_vendor_prefixes/5" do
    test "appends a re-indented block" do
      result =
        ".a {\n  color: red;\n}\n"
        |> igniter_with()
        |> Codemods.append_raw_to_rule(@path, ".a", "&:hover {\n  color: blue;\n}")
        |> content()

      assert result == ".a {\n  color: red;\n  &:hover {\n    color: blue;\n  }\n}\n"
    end

    test "adds vendor prefixes" do
      result =
        ".a {\n  user-select: none;\n}\n"
        |> igniter_with()
        |> Codemods.add_vendor_prefixes(@path, "user-select", ["-webkit-"])
        |> content()

      assert result == ".a {\n  -webkit-user-select: none;\n  user-select: none;\n}\n"
    end
  end

  describe "failure handling" do
    test "an ambiguous selector stops the installer instead of guessing" do
      igniter = igniter_with(".a {}\n.a {}\n")

      assert_raise RuntimeError, ~r/refusing to guess/, fn ->
        Codemods.set_declaration(igniter, @path, ".a", "color", "red")
      end
    end

    test "a file we refuse to patch stops the installer" do
      igniter = igniter_with(".broken {\n  color: red;\n")

      assert_raise RuntimeError, ~r/unbalanced/, fn ->
        Codemods.ensure_rule(igniter, @path, ".probe")
      end
    end
  end
end
