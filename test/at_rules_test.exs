# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.AtRulesTest do
  @moduledoc """
  Reading at-rules back whole — the query an installer needs when the *shape* of
  the user's setup decides what it should generate.

  `has_at_rule?/3` answers "is it there"; `get_at_rules/4` answers "and how was
  it configured". A generator that emits `@apply btn` into a project whose
  daisyUI is loaded as `@plugin "daisyui" { prefix: "d-" }` produces CSS that
  will not build, and the only way to know is to read the block.
  """

  use IgniterCss.CssCase, async: true

  @tailwind """
  /* the app's own stylesheet */
  @import "tailwindcss" source(none);
  @import "../vendor/app.css";
  @plugin "../vendor/heroicons";
  @plugin "daisyui" {
    prefix: "d-"; /* keeps daisyUI off our own .btn */
    exclude: rootcolor;
    logs: false;
  }
  @source "../js";

  .btn {
    color: red;
  }
  """

  describe "AtRule.body" do
    test "hands back the block verbatim, comments and all" do
      assert {:ok, [rule]} = IgniterCss.get_at_rules(@tailwind, "plugin", "daisyui")

      assert rule.body =~ ~s|prefix: "d-"; /* keeps daisyUI off our own .btn */|
      assert rule.body =~ "exclude: rootcolor;"
    end

    test "is nil for an at-rule that carries no block" do
      assert {:ok, [rule]} = IgniterCss.get_at_rules(@tailwind, "source")
      refute rule.has_block
      assert rule.body == nil
    end

    test "round-trips through ensure_at_rule_block/5" do
      assert {:ok, [rule]} = IgniterCss.get_at_rules(@tailwind, "plugin", "daisyui")
      assert {:ok, out} = IgniterCss.ensure_at_rule_block("", "plugin", ~s|"daisyui"|, rule.body)
      assert {:ok, [copied]} = IgniterCss.get_at_rules(out.source, "plugin", "daisyui")
      assert copied.declarations == rule.declarations
    end
  end

  describe "ensure_at_rule_block/5" do
    test "inserts the block into the prologue when the at-rule is absent" do
      assert {:ok, out} =
               IgniterCss.ensure_at_rule_block(@tailwind, "theme", nil, "--color-brand: red;")

      assert out.changed
      assert out.source =~ "@theme {\n  --color-brand: red;\n}"
      assert {:ok, [rule]} = IgniterCss.get_at_rules(out.source, "theme")
      assert rule.declarations == [{"--color-brand", "red"}]
    end

    test "replaces the body of the at-rule it already has, rather than adding a second" do
      assert {:ok, first} =
               IgniterCss.ensure_at_rule_block(@tailwind, "theme", nil, "--color-brand: red;")

      assert {:ok, second} =
               IgniterCss.ensure_at_rule_block(first.source, "theme", nil, "--color-brand: blue;")

      assert {:ok, [rule]} = IgniterCss.get_at_rules(second.source, "theme")
      assert rule.declarations == [{"--color-brand", "blue"}]
    end

    test "is idempotent, so a re-run produces no diff" do
      assert {:ok, first} =
               IgniterCss.ensure_at_rule_block(@tailwind, "theme", nil, "--color-brand: red;")

      assert {:ok, again} =
               IgniterCss.ensure_at_rule_block(first.source, "theme", nil, "--color-brand: red;")

      refute again.changed
      assert again.source == first.source
    end

    test "narrows to one target and leaves its siblings alone" do
      assert {:ok, out} =
               IgniterCss.ensure_at_rule_block(@tailwind, "plugin", "daisyui", ~s|prefix: "x-";|)

      assert {:ok, [rule]} = IgniterCss.get_at_rules(out.source, "plugin", "daisyui")
      assert rule.declarations == [{"prefix", ~s|"x-"|}]
      assert out.source =~ ~s|@plugin "../vendor/heroicons";|
    end

    test "refuses an at-rule that carries no block" do
      assert {:error, _} = IgniterCss.ensure_at_rule_block(@tailwind, "source", nil, "x: 1;")
    end

    test "leaves the rest of the stylesheet untouched" do
      assert {:ok, out} =
               IgniterCss.ensure_at_rule_block(@tailwind, "theme", nil, "--color-brand: red;")

      assert out.source =~ "/* the app's own stylesheet */"
      assert out.source =~ ".btn {\n  color: red;\n}"
      assert out.source =~ ~s|@import "tailwindcss" source(none);|
    end
  end

  describe "get_at_rules/4" do
    test "reads a block as declarations, in source order" do
      assert {:ok, [rule]} = IgniterCss.get_at_rules(@tailwind, "plugin", "daisyui")

      assert rule.name == "plugin"
      assert rule.target == "daisyui"
      assert rule.has_block

      assert rule.declarations == [
               {"prefix", ~s|"d-"|},
               {"exclude", "rootcolor"},
               {"logs", "false"}
             ]
    end

    test "without a target, every at-rule of that name comes back" do
      assert {:ok, plugins} = IgniterCss.get_at_rules(@tailwind, "plugin")
      assert Enum.map(plugins, & &1.target) == ["../vendor/heroicons", "daisyui"]

      assert {:ok, imports} = IgniterCss.get_at_rules(@tailwind, "import")
      assert Enum.map(imports, & &1.target) == ["tailwindcss", "../vendor/app.css"]
    end

    test "a blockless at-rule reports no declarations" do
      assert {:ok, [heroicons]} =
               IgniterCss.get_at_rules(@tailwind, "plugin", "../vendor/heroicons")

      refute heroicons.has_block
      assert heroicons.declarations == []
    end

    test "the prelude keeps what the target drops" do
      assert {:ok, [tailwind | _]} = IgniterCss.get_at_rules(@tailwind, "import")
      assert tailwind.prelude =~ "source(none)"
      assert tailwind.target == "tailwindcss"
    end

    test "the text is verbatim, comments included" do
      assert {:ok, [rule]} = IgniterCss.get_at_rules(@tailwind, "plugin", "daisyui")
      assert String.starts_with?(rule.text, ~s|@plugin "daisyui" {|)
      assert rule.text =~ "/* keeps daisyUI off our own .btn */"
      assert String.ends_with?(rule.text, "}")
    end

    test "the leading @ is optional" do
      assert IgniterCss.get_at_rules(@tailwind, "@source") ==
               IgniterCss.get_at_rules(@tailwind, "source")
    end

    test "absence is an empty list, not an error" do
      assert {:ok, []} = IgniterCss.get_at_rules(@tailwind, "plugin", "not-installed")
      assert {:ok, []} = IgniterCss.get_at_rules(@tailwind, "container")
      assert {:ok, []} = IgniterCss.get_at_rules(".a { color: red; }\n", "plugin")
    end

    test "reading never edits — the source comes back byte for byte" do
      for {name, css} <- fixtures() do
        before = css
        assert {:ok, _} = IgniterCss.get_at_rules(css, "media")
        assert css == before, "#{name}: the source was mutated by a read"
      end
    end

    test "every fixture answers rather than raising" do
      for {name, css} <- fixtures() do
        assert {:ok, list} = IgniterCss.get_at_rules(css, "import"),
               "#{name}: get_at_rules/4 failed"

        assert is_list(list)
      end
    end

    test "agrees with has_at_rule? on presence" do
      for target <- ["daisyui", "../vendor/heroicons"] do
        assert {:ok, [_]} = IgniterCss.get_at_rules(@tailwind, "plugin", target)
        assert {:ok, true} = IgniterCss.has_at_rule?(@tailwind, ~s|@plugin "#{target}";|)
      end

      assert {:ok, []} = IgniterCss.get_at_rules(@tailwind, "plugin", "missing")
      assert {:ok, false} = IgniterCss.has_at_rule?(@tailwind, ~s|@plugin "missing";|)
    end

    test "reads the prefix an installer would have to guess otherwise" do
      prefix =
        with {:ok, [rule]} <- IgniterCss.get_at_rules(@tailwind, "plugin", "daisyui"),
             {_, raw} <- List.keyfind(rule.declarations, "prefix", 0) do
          String.trim(raw, ~s|"|)
        else
          _ -> ""
        end

      assert prefix == "d-"
    end
  end
end
