# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.SelectorsTest do
  @moduledoc """
  "Can it change any line, in any class, id or tag?"

  Mirrors `native/igniter_css/tests/selectors.rs` through the NIF, so the same
  table is proven on both sides of the boundary. Each selector shape goes
  through the full lifecycle — update, append, query, remove a declaration,
  remove the rule — with comments in awkward positions throughout.
  """

  use IgniterCss.CssCase, async: true

  # {label, selector, an equivalent spelling, a near miss that must NOT match}
  @selectors [
    {"class", ".btn", ".btn", ".btn-primary"},
    {"id", "#main", "#main", "#mai"},
    {"tag", "div", "div", "div span"},
    {"universal", "*", "*", "*.x"},
    {"attribute", ~s|a[href^="https://"]|, ~s|a[href^="https://"]|, ~s|a[href^="http://"]|},
    {"attribute bare", "[data-phx-session]", "[data-phx-session]", "[data-phx]"},
    {"pseudo class", "a:hover", "a:hover", "a:focus"},
    {"pseudo element", "p::first-line", "p::first-line", "p::first-letter"},
    {"functional pseudo", "li:nth-child(2n+1)", "li:nth-child(2n+1)", "li:nth-child(2n)"},
    {"not()", "input:not([disabled])", "input:not([disabled])", "input:not([readonly])"},
    {"where()", ":where(h1, h2)", ":where(h1, h2)", ":where(h1, h3)"},
    {"root", ":root", ":root", ":host"},
    {"descendant", "nav ul li", "nav    ul   li", "nav ul"},
    {"child", ".a > .b", ".a>.b", ".a .b"},
    {"adjacent sibling", ".a + .b", ".a+.b", ".a ~ .b"},
    {"general sibling", ".a ~ .b", ".a~.b", ".a + .b"},
    {"selector list", ".a, .b", ".a,.b", ".a"},
    {"tag with class", "button.primary", "button.primary", "button"},
    {"compound chain", "#app .card > h2:first-child", "#app .card>h2:first-child",
     "#app .card h2"},
    {"escaped slash", ~S|.w-1\/2|, ~S|.w-1\/2|, ".w-1"},
    {"non ascii", ".café", ".café", ".cafe"},
    {"double class", ".a.b", ".a.b", ".a .b"}
  ]

  defp rule_for(selector) do
    """
    /* above #{selector} */
    #{selector} {
      color: red; /* trailing */
      margin: 0;
    }
    """
  end

  describe "full lifecycle" do
    for {label, selector, _equivalent, _near_miss} <- @selectors do
      test "#{label}: #{selector} can be updated, extended, queried and removed" do
        selector = unquote(selector)
        src = rule_for(selector)

        # update a value -- only the value bytes may change
        assert {:ok, %{changed: true} = updated} =
                 IgniterCss.set_declaration(src, selector, "color", "blue")

        assert updated.source =~ "color: blue; /* trailing */"
        assert updated.source =~ "/* above #{selector} */"
        assert_comments_preserved(src, updated.source)
        assert_changed_lines(src, updated.source, 2)

        # append a declaration
        assert {:ok, %{changed: true} = appended} =
                 IgniterCss.set_declaration(updated.source, selector, "padding", "1rem")

        assert appended.source =~ "padding: 1rem;"
        assert_comments_preserved(src, appended.source)

        # queries
        assert {:ok, true} = IgniterCss.has_rule?(appended.source, selector)
        assert {:ok, true} = IgniterCss.has_declaration?(appended.source, selector, "padding")
        assert {:ok, "blue"} = IgniterCss.get_declaration(appended.source, selector, "color")

        assert {:ok, [{"color", "blue"}, {"margin", "0"}, {"padding", "1rem"}]} =
                 IgniterCss.get_rule_declarations(appended.source, selector)

        # remove a declaration
        assert {:ok, %{changed: true} = removed} =
                 IgniterCss.remove_declaration(appended.source, selector, "padding")

        refute removed.source =~ "padding: 1rem;"

        # remove the whole rule
        assert {:ok, %{changed: true} = gone} = IgniterCss.remove_rule(removed.source, selector)
        assert {:ok, false} = IgniterCss.has_rule?(gone.source, selector)
      end
    end
  end

  describe "matching" do
    test "an equivalent spelling matches and can drive an edit" do
      for {label, selector, equivalent, _} <- @selectors do
        src = rule_for(selector)

        assert {:ok, true} = IgniterCss.has_rule?(src, equivalent),
               "#{label}: #{equivalent} should match #{selector}"

        assert {:ok, %{changed: true}} =
                 IgniterCss.set_declaration(src, equivalent, "color", "green"),
               "#{label}: editing via #{equivalent} did nothing"
      end
    end

    test "a near miss never matches" do
      for {label, selector, _, near_miss} <- @selectors do
        src = rule_for(selector)

        assert {:ok, false} = IgniterCss.has_rule?(src, near_miss),
               "#{label}: #{near_miss} must not match #{selector}"

        assert {:ok, %{changed: false, source: ^src}} = IgniterCss.remove_rule(src, near_miss),
               "#{label}: #{near_miss} removed something"
      end
    end

    test "every shape is idempotent" do
      for {label, selector, _, _} <- @selectors do
        src = rule_for(selector)

        assert_idempotent(src, &IgniterCss.set_declaration(&1, selector, "padding", "1rem"))
        assert_idempotent(src, &IgniterCss.remove_rule(&1, selector))
        assert_idempotent(src, &IgniterCss.remove_declaration(&1, selector, "color"))
        _ = label
      end
    end
  end

  describe "property shapes" do
    @properties [
      {"standard", "color", "red"},
      {"hyphenated", "background-color", "#fff"},
      {"custom property", "--brand", "#4f46e5"},
      {"vendor prefixed", "-webkit-user-select", "none"},
      {"shorthand", "margin", "0 auto 10px"},
      {"function value", "background", "var(--x, #fff)"},
      {"multi function", "transform", "translate(1px) rotate(2deg)"},
      {"url with a semicolon", "background-image", "url(data:image/svg+xml;base64,AA==)"},
      {"string value", "content", ~s|"→ ✨"|},
      {"grid template", "grid-template-columns", "minmax(12rem, 1fr) 3fr"},
      # Deliberately not `z-index`: the base rule already sets it, which would
      # make this an update rather than the append the assertion expects.
      {"unitless number", "order", "10"}
    ]

    test "every property shape sets, reads back and removes cleanly" do
      for {label, property, value} <- @properties do
        src = ".x {\n  z-index: 1;\n}\n"

        assert {:ok, %{changed: true} = out} =
                 IgniterCss.set_declaration(src, ".x", property, value),
               "#{label}: set failed"

        assert {:ok, ^value} = IgniterCss.get_declaration(out.source, ".x", property),
               "#{label}: value did not round-trip"

        assert {:ok, %{source: ^src}} =
                 IgniterCss.remove_declaration(out.source, ".x", property),
               "#{label}: removal did not restore the original"
      end
    end

    test "the important flag is set, read back and cleared" do
      src = ".x {\n  z-index: 1;\n}\n"

      assert {:ok, out} = IgniterCss.set_declaration(src, ".x", "color", "red", important: true)
      assert out.source =~ "color: red !important;"
      assert {:ok, "red !important"} = IgniterCss.get_declaration(out.source, ".x", "color")

      # An update with no opinion keeps the flag.
      assert {:ok, kept} = IgniterCss.set_declaration(out.source, ".x", "color", "blue")
      assert kept.source =~ "color: blue !important;"

      # An explicit false clears it.
      assert {:ok, cleared} =
               IgniterCss.set_declaration(kept.source, ".x", "color", "blue", important: false)

      assert cleared.source =~ "color: blue;"
      refute cleared.source =~ "!important"
    end

    test "a property is matched case-insensitively but a custom property is not" do
      assert {:ok, "red"} = IgniterCss.get_declaration(".a { COLOR: red; }", ".a", "color")

      assert {:ok, nil} =
               IgniterCss.get_declaration(":root { --Brand: red; }", ":root", "--brand")

      assert {:ok, "red"} =
               IgniterCss.get_declaration(":root { --Brand: red; }", ":root", "--Brand")
    end
  end

  describe "scoping" do
    test "a nested rule is not reachable at the top level" do
      src = "@media print {\n  .b { color: red; }\n}\n"

      assert {:ok, false} = IgniterCss.has_rule?(src, ".b")
      assert {:ok, %{changed: false}} = IgniterCss.remove_rule(src, ".b")
      assert {:error, reason} = IgniterCss.set_declaration(src, ".b", "color", "blue")
      assert reason =~ "not found"
    end

    test "the same selector at two scopes only touches the top-level one" do
      src = ".b { color: red; }\n@media print {\n  .b { color: green; }\n}\n"

      assert {:ok, out} = IgniterCss.set_declaration(src, ".b", "color", "blue")
      assert out.source == ".b { color: blue; }\n@media print {\n  .b { color: green; }\n}\n"
    end

    test "duplicate top-level rules are refused rather than guessed" do
      src = ".b { color: red; }\n.b { margin: 0; }\n"
      assert {:error, reason} = IgniterCss.set_declaration(src, ".b", "color", "blue")
      assert reason =~ "matches 2 top-level rules"
    end
  end
end
