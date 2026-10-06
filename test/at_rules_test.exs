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

  describe "ensure_at_rule_declarations/5" do
    # A project's `@theme` holds the tokens a library installs AND the project's
    # own. Replacing the body, as `ensure_at_rule_block/5` does, wiped the second
    # on every install.
    @shared_theme """
    @import "tailwindcss";

    @theme {
      /* the project's own */
      --font-caveat: "caveat", cursive;
      --color-brand: #1eb0ff;

      --color-primary-light: red;
    }
    """

    @library_tokens """
      --color-primary-light: var(--primary-light);
      --color-primary-gradient: linear-gradient(
        to right,
        var(--primary-light)
      );
    """

    test "sets the library's declarations and keeps the project's own, comments and all" do
      assert {:ok, out} =
               IgniterCss.ensure_at_rule_declarations(
                 @shared_theme,
                 "theme",
                 nil,
                 @library_tokens
               )

      assert out.changed

      assert out.source == """
             @import "tailwindcss";

             @theme {
               /* the project's own */
               --font-caveat: "caveat", cursive;
               --color-brand: #1eb0ff;

               --color-primary-light: var(--primary-light);
               --color-primary-gradient: linear-gradient(
                 to right,
                 var(--primary-light)
               );
             }
             """
    end

    test "re-running changes nothing" do
      assert {:ok, first} =
               IgniterCss.ensure_at_rule_declarations(
                 @shared_theme,
                 "theme",
                 nil,
                 @library_tokens
               )

      assert {:ok, again} =
               IgniterCss.ensure_at_rule_declarations(first.source, "theme", nil, @library_tokens)

      refute again.changed
      assert again.source == first.source
    end

    test "a value already set is not rewritten, however its lines are spaced" do
      css = "@theme {\n  --g: linear-gradient(to right,\n      red);\n}\n"

      assert {:ok, out} =
               IgniterCss.ensure_at_rule_declarations(
                 css,
                 "theme",
                 nil,
                 "--g: linear-gradient(to right, red);"
               )

      refute out.changed
    end

    test "agrees with get_at_rules on what the block now holds" do
      assert {:ok, out} =
               IgniterCss.ensure_at_rule_declarations(
                 @shared_theme,
                 "theme",
                 nil,
                 "--color-a: blue;"
               )

      assert {:ok, [rule]} = IgniterCss.get_at_rules(out.source, "theme")

      assert rule.declarations == [
               {"--font-caveat", ~s|"caveat", cursive|},
               {"--color-brand", "#1eb0ff"},
               {"--color-primary-light", "red"},
               {"--color-a", "blue"}
             ]
    end

    test "inserts the whole rule, as ensure_at_rule_block does, when the at-rule is absent" do
      assert IgniterCss.ensure_at_rule_declarations(
               @tailwind,
               "theme",
               nil,
               "--color-brand: red;"
             ) ==
               IgniterCss.ensure_at_rule_block(@tailwind, "theme", nil, "--color-brand: red;")
    end

    test "narrows to one target and leaves its siblings and comments alone" do
      assert {:ok, out} =
               IgniterCss.ensure_at_rule_declarations(
                 @tailwind,
                 "plugin",
                 "daisyui",
                 ~s|prefix: "x-"; themes: false;|
               )

      assert {:ok, [rule]} = IgniterCss.get_at_rules(out.source, "plugin", "daisyui")

      assert rule.declarations == [
               {"prefix", ~s|"x-"|},
               {"exclude", "rootcolor"},
               {"logs", "false"},
               {"themes", "false"}
             ]

      assert out.source =~ ~s|prefix: "x-"; /* keeps daisyUI off our own .btn */|
      assert out.source =~ ~s|@plugin "../vendor/heroicons";|
    end

    test "refuses what is not a plain list of declarations" do
      for declarations <- [".x { color: red; }", "@media print { a: 1; }", "color red;"] do
        assert {:error, _reason} =
                 IgniterCss.ensure_at_rule_declarations(@shared_theme, "theme", nil, declarations),
               "accepted #{inspect(declarations)}"
      end
    end

    test "refuses an at-rule that carries no block" do
      assert {:error, _} =
               IgniterCss.ensure_at_rule_declarations(@tailwind, "source", nil, "x: 1;")
    end

    test "every fixture answers rather than raising, and a second run changes nothing" do
      for {name, css} <- fixtures() do
        case IgniterCss.ensure_at_rule_declarations(css, "theme", nil, "--mishka-probe: 1;") do
          {:ok, out} ->
            assert {:ok, again} =
                     IgniterCss.ensure_at_rule_declarations(
                       out.source,
                       "theme",
                       nil,
                       "--mishka-probe: 1;"
                     )

            refute again.changed, "#{name}: a second run changed the file"

          {:error, reason} ->
            assert is_binary(reason), "#{name}: #{inspect(reason)}"
        end
      end
    end
  end

  describe "remove_at_rule_declarations/5" do
    # Uninstalling takes back the tokens a library put into the project's
    # `@theme`, and nothing the project wrote there.
    @project_theme """
    @import "tailwindcss";

    @theme {
      /* the project's own */
      --font-caveat: "caveat", cursive;
      --color-brand: #1eb0ff;
    }
    """

    @library_tokens """
      --color-primary-light: var(--primary-light);
      --color-primary-gradient: linear-gradient(
        to right,
        var(--primary-light)
      );
    """

    defp install(css, matching \\ "") do
      {:ok, out} = IgniterCss.ensure_at_rule_declarations(css, "theme", matching, @library_tokens)
      out.source
    end

    defp uninstall(css, matching \\ "") do
      IgniterCss.remove_at_rule_declarations(css, "theme", matching, @library_tokens)
    end

    test "takes back what ensure_at_rule_declarations set, and gives the project's block back" do
      installed = install(@project_theme)
      assert installed =~ "--color-primary-gradient"

      assert {:ok, out} = uninstall(installed)
      assert out.changed
      assert out.source == @project_theme
    end

    test "a block it inserted goes again, and the file is as it was" do
      css = ~s|@import "tailwindcss";\n\n.btn {\n  color: red;\n}\n|
      installed = install(css)
      assert installed =~ "@theme {"

      assert {:ok, out} = uninstall(installed)
      assert out.source == css
    end

    test "a value changed since is the project's now, and stays" do
      installed = String.replace(install(@project_theme), "var(--primary-light);", "red;")

      assert {:ok, out} = uninstall(installed)
      assert {:ok, [rule]} = IgniterCss.get_at_rules(out.source, "theme")

      assert rule.declarations == [
               {"--font-caveat", ~s|"caveat", cursive|},
               {"--color-brand", "#1eb0ff"},
               {"--color-primary-light", "red"}
             ]
    end

    test "a value is compared token for token, however its lines are spaced" do
      css = "@theme {\n  --keep: 1;\n  --g: linear-gradient(to right,\n      red);\n}\n"

      assert {:ok, out} =
               IgniterCss.remove_at_rule_declarations(
                 css,
                 "theme",
                 "",
                 "--g: linear-gradient(to right, red);"
               )

      assert out.source == "@theme {\n  --keep: 1;\n}\n"
    end

    test "a removed declaration takes the comments it owns, and the block keeps its own" do
      css = """
      @theme {
        /* ===== tokens ===== */
        --keep: 1;
        /* the library's */
        --a: 1; /* trailing */
      }
      """

      assert {:ok, out} = IgniterCss.remove_at_rule_declarations(css, "theme", "", "--a: 1;")
      assert out.source == "@theme {\n  /* ===== tokens ===== */\n  --keep: 1;\n}\n"
    end

    test "a block left empty goes, with the comment above it" do
      css =
        ~s|@import "x";\n\n/* library tokens */\n@theme {\n  --a: 1;\n}\n\n.btn { color: red; }\n|

      assert {:ok, out} = IgniterCss.remove_at_rule_declarations(css, "theme", "", "--a: 1;")
      assert out.source == ~s|@import "x";\n\n.btn { color: red; }\n|
    end

    test ~s|"" is the plain @theme; @theme inline is left alone| do
      css = "@theme inline {\n  --font-sans: var(--font-geist);\n}\n"
      installed = install(css)

      assert {:ok, [inline]} = IgniterCss.get_at_rules(installed, "theme", "inline")
      assert inline.declarations == [{"--font-sans", "var(--font-geist)"}]
      assert {:ok, [plain]} = IgniterCss.get_at_rules(installed, "theme", "")
      assert length(plain.declarations) == 2

      assert {:ok, out} = uninstall(installed)
      assert out.source == css
    end

    test "nil is the first block of that name, whatever follows it" do
      css = "@theme inline {\n  --a: 1;\n}\n\n@theme {\n  --a: 1;\n}\n"

      assert {:ok, out} = IgniterCss.remove_at_rule_declarations(css, "theme", nil, "--a: 1;")
      assert out.source == "@theme {\n  --a: 1;\n}\n"
    end

    test "nothing to remove is no change" do
      for {css, name} <- [
            {~s|@import "x";\n|, "theme"},
            {"@theme {\n  --b: 2;\n}\n", "theme"},
            {@tailwind, "source"}
          ] do
        assert {:ok, out} = IgniterCss.remove_at_rule_declarations(css, name, nil, "--a: 1;")
        refute out.changed
        assert out.source == css
      end
    end

    test "removing twice is removing once" do
      {:ok, once} = uninstall(install(@project_theme))
      assert {:ok, twice} = uninstall(once.source)
      refute twice.changed
    end

    test "refuses what is not a plain list of declarations" do
      for declarations <- [".x { color: red; }", "@media print { a: 1; }", "color red;"] do
        assert {:error, _reason} =
                 IgniterCss.remove_at_rule_declarations(
                   @project_theme,
                   "theme",
                   nil,
                   declarations
                 ),
               "accepted #{inspect(declarations)}"
      end
    end

    test "every fixture answers rather than raising, and a probe set and taken back leaves it as it was" do
      for {name, css} <- fixtures() do
        case IgniterCss.ensure_at_rule_declarations(css, "theme", "", "--mishka-probe: 1;") do
          {:ok, set} ->
            assert {:ok, out} =
                     IgniterCss.remove_at_rule_declarations(
                       set.source,
                       "theme",
                       "",
                       "--mishka-probe: 1;"
                     )

            assert out.source == css, "#{name}: the file did not come back"

          {:error, reason} ->
            assert is_binary(reason), "#{name}: #{inspect(reason)}"
        end
      end
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

    test "a target matches however it is quoted" do
      assert IgniterCss.get_at_rules(@tailwind, "plugin", ~s|"daisyui"|) ==
               IgniterCss.get_at_rules(@tailwind, "plugin", "daisyui")
    end

    test "an at-rule without a target is matched by its prelude, and \"\" by none" do
      css = "@theme inline {\n  --font: x;\n}\n@theme {\n  --a: 1;\n}\n"

      assert {:ok, [inline]} = IgniterCss.get_at_rules(css, "theme", "inline")
      assert inline.prelude == "inline"
      assert {:ok, [plain]} = IgniterCss.get_at_rules(css, "theme", "")
      assert plain.declarations == [{"--a", "1"}]
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
