# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCssTest do
  use IgniterCss.CssCase, async: true

  doctest IgniterCss

  alias IgniterCss.Outcome

  describe "ensure_at_rule/3" do
    test "inserts into an empty file" do
      assert {:ok, %Outcome{source: ~s|@plugin "daisyui";\n|, changed: true}} =
               IgniterCss.ensure_at_rule("", ~s|@plugin "daisyui";|)
    end

    test "inserts after the last at-rule of the same name" do
      css = ~s|@import "a";\n@import "b";\n\n.x { color: red; }\n|

      assert {:ok, %Outcome{source: out}} = IgniterCss.ensure_at_rule(css, ~s|@import "c";|)
      assert out == ~s|@import "a";\n@import "b";\n@import "c";\n\n.x { color: red; }\n|
    end

    test "inserts at the end of the prologue when the family is new" do
      css = ~s|@import "tailwindcss";\n@source "../js";\n\n.x { color: red; }\n|

      assert {:ok, %Outcome{source: out}} = IgniterCss.ensure_at_rule(css, ~s|@plugin "d";|)

      assert out ==
               ~s|@import "tailwindcss";\n@source "../js";\n@plugin "d";\n\n.x { color: red; }\n|
    end

    test "never places an @import after a style rule" do
      assert {:ok, %Outcome{source: out}} =
               IgniterCss.ensure_at_rule(".x { color: red; }\n", ~s|@import "b";|)

      assert String.starts_with?(out, ~s|@import "b";\n.x|)
    end

    test "an equivalent rule with the same target is not duplicated" do
      css = ~s|@import "tailwindcss" source(none);\n|

      assert {:ok, %Outcome{changed: false, source: ^css}} =
               IgniterCss.ensure_at_rule(css, ~s|@import "tailwindcss";|)
    end

    test "quoting style does not create a duplicate" do
      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.ensure_at_rule(~s|@plugin '../vendor/x';\n|, ~s|@plugin "../vendor/x";|)
    end

    test "is idempotent" do
      assert_idempotent(
        ~s|@import "a";\n.x {}\n|,
        &IgniterCss.ensure_at_rule(&1, ~s|@plugin "p";|)
      )
    end

    test "keeps every comment" do
      css = ~s|/* one */\n@import "a"; /* two */\n/* three */\n.x {}\n|
      {:ok, out} = IgniterCss.ensure_at_rule(css, ~s|@import "b";|)
      assert_comments_preserved(css, out.source)
    end

    test "adds only one line" do
      css = fixture("phoenix_app.css")
      {:ok, out} = IgniterCss.ensure_at_rule(css, ~s|@plugin "probe";|)
      assert_changed_lines(css, out.source, 1)
    end

    test "rejects text that is not an at-rule" do
      assert {:error, reason} = IgniterCss.ensure_at_rule("", ".a { color: red; }")
      assert reason =~ "at-rule"
    end

    test "rejects an unbalanced at-rule line" do
      assert {:error, _} = IgniterCss.ensure_at_rule("", ~s|@plugin "x" {|)
    end
  end

  describe "remove_at_rule/4" do
    test "removes only the matching rule" do
      css = ~s|@import "a";\n@import "b";\n.x {}\n|

      assert {:ok, %Outcome{source: ~s|@import "b";\n.x {}\n|}} =
               IgniterCss.remove_at_rule(css, "import", "a")
    end

    test "removes every rule of the name when unfiltered" do
      css = ~s|@import "a";\n@import "b";\n.x {}\n|
      assert {:ok, %Outcome{source: ".x {}\n"}} = IgniterCss.remove_at_rule(css, "import")
    end

    test "takes the adjacent comment but keeps a section header" do
      css = ~s|/* ===== Imports ===== */\n/* the app css */\n@import "a";\n@import "b";\n|

      assert {:ok, %Outcome{source: out}} = IgniterCss.remove_at_rule(css, "import", "a")
      assert out == ~s|/* ===== Imports ===== */\n@import "b";\n|
    end

    test "removing something absent is a no-op" do
      assert {:ok, %Outcome{changed: false}} = IgniterCss.remove_at_rule(".x {}\n", "import", "a")
    end
  end

  describe "has_at_rule?/3" do
    test "agrees with ensure_at_rule" do
      css = ~s|@plugin "a";\n|
      assert {:ok, true} = IgniterCss.has_at_rule?(css, ~s|@plugin "a";|)
      assert {:ok, false} = IgniterCss.has_at_rule?(css, ~s|@plugin "b";|)
    end
  end

  describe "add_import/4 and remove_import/3" do
    test "quotes a relative path and wraps an absolute url" do
      assert {:ok, %Outcome{source: ~s|@import "styles.css";\n|}} =
               IgniterCss.add_import("", "styles.css")

      assert {:ok, %Outcome{source: ~s|@import url("https://x/y.css");\n|}} =
               IgniterCss.add_import("", "https://x/y.css")
    end

    test "carries a media query" do
      assert {:ok, %Outcome{source: out}} =
               IgniterCss.add_import("", "m.css", "screen and (max-width: 768px)")

      assert out == ~s|@import "m.css" screen and (max-width: 768px);\n|
    end

    test "matches a url written either way when removing" do
      css = ~s|@import url("/a.css");\n@import "b";\n|
      assert {:ok, %Outcome{source: ~s|@import "b";\n|}} = IgniterCss.remove_import(css, "/a.css")
    end

    test "rejects an unquotable url" do
      assert {:error, _} = IgniterCss.add_import("", ~s|a"b|)
      assert {:error, _} = IgniterCss.add_import("", "   ")
    end
  end

  describe "ensure_rule/4" do
    test "creates a missing rule at the end" do
      assert {:ok, %Outcome{source: ".a { color: red; }\n\n.b {\n}\n"}} =
               IgniterCss.ensure_rule(".a { color: red; }\n", ".b")
    end

    test "seeds the new rule with declarations" do
      assert {:ok, %Outcome{source: ".b {\n  color: red;\n  margin: 0;\n}\n"}} =
               IgniterCss.ensure_rule("", ".b", "color: red; margin: 0")
    end

    test "does not recreate an existing rule" do
      css = ".b {\n  color: red;\n}\n"
      assert {:ok, %Outcome{changed: false, source: ^css}} = IgniterCss.ensure_rule(css, ".b")
    end

    test "matches a rule written with different spacing" do
      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.ensure_rule(".a   >   .b { color: red; }\n", ".a>.b")
    end

    test "follows the file's indent and newline style" do
      css = ".a {\r\n\tcolor: red;\r\n}\r\n"
      assert {:ok, %Outcome{source: out}} = IgniterCss.ensure_rule(css, ".b", "margin: 0")
      assert out == ".a {\r\n\tcolor: red;\r\n}\r\n\r\n.b {\r\n\tmargin: 0;\r\n}\r\n"
    end

    test "a file without a trailing newline keeps not having one" do
      assert {:ok, %Outcome{source: ".a {}\n\n.b {\n}"}} = IgniterCss.ensure_rule(".a {}", ".b")
    end

    test "is idempotent" do
      assert_idempotent(".a {}\n", &IgniterCss.ensure_rule(&1, ".b"))
    end

    test "rejects a selector containing braces" do
      assert {:error, _} = IgniterCss.ensure_rule("", ".a { }")
      assert {:error, _} = IgniterCss.ensure_rule("", "   ")
    end
  end

  describe "remove_rule/3" do
    test "removes the rule and its line" do
      assert {:ok, %Outcome{source: ".a {}\n.c {}\n"}} =
               IgniterCss.remove_rule(".a {}\n.b {}\n.c {}\n", ".b")
    end

    test "removes the comment directly above" do
      assert {:ok, %Outcome{source: ".a {}\n\n.c {}\n"}} =
               IgniterCss.remove_rule(".a {}\n\n/* about b */\n.b {}\n\n.c {}\n", ".b")
    end

    test "keeps a section header" do
      css = "/* ===== Utilities ===== */\n.b {}\n.c {}\n"

      assert {:ok, %Outcome{source: "/* ===== Utilities ===== */\n.c {}\n"}} =
               IgniterCss.remove_rule(css, ".b")
    end

    test "does not reach into a media block" do
      css = "@media print {\n  .b { color: red; }\n}\n"
      assert {:ok, %Outcome{changed: false}} = IgniterCss.remove_rule(css, ".b")
    end

    test "removing an absent rule is a no-op" do
      assert {:ok, %Outcome{changed: false}} = IgniterCss.remove_rule(".a {}\n", ".zz")
    end
  end

  describe "set_declaration/5" do
    test "updates an existing value" do
      assert {:ok, %Outcome{source: ".a {\n  color: blue;\n}\n"}} =
               IgniterCss.set_declaration(".a {\n  color: red;\n}\n", ".a", "color", "blue")
    end

    test "touches only the value bytes, preserving an inline comment" do
      css = ".a {\n  color: red; /* the brand */\n  margin: 0;\n}\n"
      assert {:ok, %Outcome{source: out}} = IgniterCss.set_declaration(css, ".a", "color", "blue")
      assert out == ".a {\n  color: blue; /* the brand */\n  margin: 0;\n}\n"
      assert_changed_lines(css, out, 2)
    end

    test "preserves an existing !important by default" do
      assert {:ok, %Outcome{source: ".a { color: blue !important; }"}} =
               IgniterCss.set_declaration(".a { color: red !important; }", ".a", "color", "blue")
    end

    test "can add and remove the !important flag" do
      assert {:ok, %Outcome{source: ".a { color: blue !important; }"}} =
               IgniterCss.set_declaration(".a { color: red; }", ".a", "color", "blue",
                 important: true
               )

      assert {:ok, %Outcome{source: ".a { color: blue; }"}} =
               IgniterCss.set_declaration(
                 ".a { color: red   !important; }",
                 ".a",
                 "color",
                 "blue",
                 important: false
               )
    end

    test "appends a missing property in the file's own style" do
      assert {:ok, %Outcome{source: ".a {\n\tcolor: red;\n\tmargin: 0;\n}\n"}} =
               IgniterCss.set_declaration(".a {\n\tcolor: red;\n}\n", ".a", "margin", "0")
    end

    test "keeps a single-line rule on one line" do
      assert {:ok, %Outcome{source: ".a { color: red; margin: 0; }\n"}} =
               IgniterCss.set_declaration(".a { color: red; }\n", ".a", "margin", "0")
    end

    test "updates a custom property" do
      assert {:ok, %Outcome{source: ":root {\n  --brand: #000;\n}\n"}} =
               IgniterCss.set_declaration(
                 ":root {\n  --brand: #fff;\n}\n",
                 ":root",
                 "--brand",
                 "#000"
               )
    end

    test "a missing rule errors by default and can be created on request" do
      assert {:error, reason} = IgniterCss.set_declaration(".a {}\n", ".zz", "color", "red")
      assert reason =~ "not found"

      assert {:ok, %Outcome{source: ".a {}\n\n.zz {\n  color: red;\n}\n"}} =
               IgniterCss.set_declaration(".a {}\n", ".zz", "color", "red", create_rule: true)
    end

    test "an ambiguous selector errors rather than guessing" do
      assert {:error, reason} = IgniterCss.set_declaration(".a {}\n.a {}\n", ".a", "color", "red")
      assert reason =~ "matches 2 top-level rules"
      assert reason =~ "refusing to guess"
    end

    test "writing back the same value reports no change" do
      css = ".a {\n  color: red;\n}\n"

      assert {:ok, %Outcome{changed: false, source: ^css}} =
               IgniterCss.set_declaration(css, ".a", "color", "red")
    end

    test "accepts a url value containing a semicolon" do
      assert {:ok, %Outcome{source: out}} =
               IgniterCss.set_declaration(
                 ".a { background: none; }",
                 ".a",
                 "background",
                 "url(data:image/svg+xml;base64,AA==)"
               )

      assert out == ".a { background: url(data:image/svg+xml;base64,AA==); }"
    end

    test "rejects a value carrying its own delimiters" do
      for {property, value} <- [
            {"color", "red; margin: 0"},
            {"color", "red !important"},
            {"color:x", "red"},
            {"", "red"},
            {"color", "  "}
          ] do
        assert {:error, _} = IgniterCss.set_declaration(".a{}", ".a", property, value)
      end
    end

    test "is idempotent" do
      assert_idempotent(
        ".a {\n  color: red;\n}\n",
        &IgniterCss.set_declaration(&1, ".a", "margin", "0")
      )
    end
  end

  describe "remove_declaration/4" do
    test "removes the declaration and its line" do
      assert {:ok, %Outcome{source: ".a {\n  margin: 0;\n}\n"}} =
               IgniterCss.remove_declaration(
                 ".a {\n  color: red;\n  margin: 0;\n}\n",
                 ".a",
                 "color"
               )
    end

    test "takes the trailing comment with it" do
      assert {:ok, %Outcome{source: ".a {\n  margin: 0;\n}\n"}} =
               IgniterCss.remove_declaration(
                 ".a {\n  color: red; /* legacy */\n  margin: 0;\n}\n",
                 ".a",
                 "color"
               )
    end

    test "keeps a comment separated by a blank line" do
      css = ".a {\n  /* about the block */\n\n  color: red;\n  margin: 0;\n}\n"

      assert {:ok, %Outcome{source: ".a {\n  /* about the block */\n\n  margin: 0;\n}\n"}} =
               IgniterCss.remove_declaration(css, ".a", "color")
    end

    test "removes every copy of a repeated property" do
      assert {:ok, %Outcome{source: ".a {\n  margin: 0;\n}\n"}} =
               IgniterCss.remove_declaration(
                 ".a {\n  color: red;\n  margin: 0;\n  color: blue;\n}\n",
                 ".a",
                 "color"
               )
    end

    test "removing from an absent rule is a no-op, not an error" do
      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.remove_declaration(".a { color: red; }", ".zz", "color")
    end

    test "is idempotent" do
      assert_idempotent(
        ".a {\n  color: red;\n  margin: 0;\n}\n",
        &IgniterCss.remove_declaration(&1, ".a", "color")
      )
    end
  end

  describe "append_raw_to_rule/4" do
    test "re-indents a multi-line block" do
      assert {:ok, %Outcome{source: out}} =
               IgniterCss.append_raw_to_rule(
                 ".a {\n  color: red;\n}\n",
                 ".a",
                 "&:hover {\n  color: blue;\n}"
               )

      assert out == ".a {\n  color: red;\n  &:hover {\n    color: blue;\n  }\n}\n"
    end

    test "is a no-op when the text is already present" do
      assert_idempotent(
        ".a {\n  color: red;\n}\n",
        &IgniterCss.append_raw_to_rule(&1, ".a", "margin: 0;")
      )
    end

    test "rejects unbalanced text" do
      assert {:error, _} = IgniterCss.append_raw_to_rule(".a {}\n", ".a", "&:hover {")
      assert {:error, _} = IgniterCss.append_raw_to_rule(".a {}\n", ".a", "   ")
    end
  end

  describe "replace_rule_body/4" do
    test "replaces a multi-line body" do
      assert {:ok, %Outcome{source: ".a {\n  padding: 1px;\n  color: blue;\n}\n"}} =
               IgniterCss.replace_rule_body(
                 ".a {\n  color: red;\n  margin: 0;\n}\n",
                 ".a",
                 "padding: 1px; color: blue"
               )
    end

    test "errors on a missing or ambiguous selector" do
      assert {:error, _} = IgniterCss.replace_rule_body(".a {}\n", ".zz", "color: red")
      assert {:error, _} = IgniterCss.replace_rule_body(".a {}\n.a {}\n", ".a", "color: red")
    end
  end

  describe "add_vendor_prefixes/4" do
    test "adds prefixes above the standard property" do
      assert {:ok, %Outcome{source: out}} =
               IgniterCss.add_vendor_prefixes(
                 ".a {\n  user-select: none;\n}\n",
                 "user-select",
                 ["-webkit-", "-moz-"]
               )

      assert out ==
               ".a {\n  -webkit-user-select: none;\n  -moz-user-select: none;\n  user-select: none;\n}\n"
    end

    test "skips a prefix that is already present" do
      css = ".a {\n  -webkit-user-select: none;\n  user-select: none;\n}\n"

      assert {:ok, %Outcome{source: out}} =
               IgniterCss.add_vendor_prefixes(css, "user-select", ["-webkit-", "-moz-"])

      assert out ==
               ".a {\n  -webkit-user-select: none;\n  -moz-user-select: none;\n  user-select: none;\n}\n"
    end

    test "carries the !important flag onto the copies" do
      assert {:ok, %Outcome{source: out}} =
               IgniterCss.add_vendor_prefixes(
                 ".a {\n  user-select: none !important;\n}\n",
                 "user-select",
                 ["-webkit-"]
               )

      assert out =~ "-webkit-user-select: none !important;"
    end

    test "an empty prefix list is a no-op" do
      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.add_vendor_prefixes(".a { user-select: none; }", "user-select", [])
    end

    test "is idempotent" do
      assert_idempotent(
        ".a {\n  user-select: none;\n}\n",
        &IgniterCss.add_vendor_prefixes(&1, "user-select", ["-webkit-", "-moz-"])
      )
    end
  end

  describe "sort_properties/2" do
    test "sorts declarations and moves their comments with them" do
      css = ".a {\n  /* about z */\n  z-index: 1;\n  color: red; /* about c */\n}\n"
      assert {:ok, %Outcome{source: out}} = IgniterCss.sort_properties(css)
      assert out == ".a {\n  color: red; /* about c */\n  /* about z */\n  z-index: 1;\n}\n"
      assert_comments_preserved(css, out)
    end

    test "leaves a block it cannot rearrange safely alone, and says so" do
      css = ".a { z-index: 1; color: red; }\n"

      assert {:ok, %Outcome{changed: false, diagnostics: [message]}} =
               IgniterCss.sort_properties(css)

      assert message =~ "unsorted"
    end

    test "is idempotent" do
      assert_idempotent(
        ".a {\n  z-index: 1;\n  color: red;\n  background: blue;\n}\n",
        &IgniterCss.sort_properties/1
      )
    end
  end

  describe "remove_duplicates/2" do
    test "drops a shadowed declaration" do
      assert {:ok, %Outcome{source: ".a {\n  margin: 0;\n  color: blue;\n}\n"}} =
               IgniterCss.remove_duplicates(
                 ".a {\n  color: red;\n  margin: 0;\n  color: blue;\n}\n"
               )
    end

    test "keeps an !important a later plain declaration cannot override" do
      css = ".a {\n  color: red !important;\n  color: blue;\n}\n"
      assert {:ok, %Outcome{changed: false}} = IgniterCss.remove_duplicates(css)
    end

    test "drops an identical duplicated rule but keeps a differing one" do
      assert {:ok, %Outcome{source: ".b {}\n\n.a {\n  color: red;\n}\n"}} =
               IgniterCss.remove_duplicates(
                 ".a {\n  color: red;\n}\n\n.b {}\n\n.a {\n  color: red;\n}\n"
               )

      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.remove_duplicates(".a { color: red; }\n.a { margin: 0; }\n")
    end

    test "each half can be switched off" do
      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.remove_duplicates(".a {\n  color: red;\n  color: blue;\n}\n",
                 declarations: false
               )

      assert {:ok, %Outcome{changed: false}} =
               IgniterCss.remove_duplicates(".a { color: red; }\n.a { color: red; }\n",
                 rules: false
               )
    end
  end

  describe "queries" do
    test "has_rule? is top-level and normalised" do
      assert {:ok, true} = IgniterCss.has_rule?(".a  >  .b {}\n", ".a>.b")
      assert {:ok, false} = IgniterCss.has_rule?("@media print { .b {} }\n", ".b")
    end

    test "has_rule? does not match one member of a selector list" do
      assert {:ok, false} = IgniterCss.has_rule?(".a, .b { color: red; }", ".a")
      assert {:ok, true} = IgniterCss.has_rule?(".a, .b { color: red; }", ".a, .b")
    end

    test "has_rule? does not match a substring" do
      assert {:ok, false} = IgniterCss.has_rule?(".header-inner { color: red; }", ".header")
    end

    test "list_selectors returns selectors as written" do
      assert {:ok, [".a,\n.b", "#c"]} =
               IgniterCss.list_selectors(".a,\n.b { color: red; }\n#c {}\n")
    end

    test "get_declaration and has_declaration? agree" do
      css = ".a { color: red; }"
      assert {:ok, "red"} = IgniterCss.get_declaration(css, ".a", "color")
      assert {:ok, nil} = IgniterCss.get_declaration(css, ".a", "margin")
      assert {:ok, nil} = IgniterCss.get_declaration(css, ".zz", "color")
      assert {:ok, true} = IgniterCss.has_declaration?(css, ".a", "color")
      assert {:ok, false} = IgniterCss.has_declaration?(css, ".a", "margin")
    end

    test "get_rule_declarations returns pairs in source order" do
      assert {:ok, [{"color", "red"}, {"margin", "0 auto"}]} =
               IgniterCss.get_rule_declarations(
                 ".a {\n  color: red;\n  margin: 0 auto;\n}\n",
                 ".a"
               )

      assert {:ok, nil} = IgniterCss.get_rule_declarations(".a {}", ".zz")
    end
  end

  describe "analyze/2" do
    test "counts the basics" do
      css = """
      /* c */
      @import "x";
      .a, .b {
        color: red;
        margin: 0 !important;
      }
      #c {
        --x: 1;
      }
      @media print {
        .d { color: blue; }
      }
      """

      assert {:ok, a} = IgniterCss.analyze(css)
      assert a.rules_count == 3
      assert a.top_level_rules_count == 2
      assert a.selectors_count == 4
      assert a.declarations_count == 4
      assert a.imports_count == 1
      assert a.media_queries_count == 1
      assert a.comments_count == 1
      assert a.important_count == 1
      assert a.custom_properties_count == 1
    end

    test "ranks properties by frequency" do
      assert {:ok, a} = IgniterCss.analyze(".a { color: red; }\n.b { color: blue; margin: 0; }\n")
      assert a.property_frequency == [{"color", 2}, {"margin", 1}]
    end

    test "analyses an empty sheet" do
      assert {:ok, %IgniterCss.Analysis{rules_count: 0, declarations_count: 0}} =
               IgniterCss.analyze("")
    end
  end

  describe "validate/2" do
    test "accepts valid css and Tailwind v4" do
      assert {:ok, %{valid: true}} = IgniterCss.validate(".a { color: red; }\n")
      assert {:ok, %{valid: true}} = IgniterCss.validate(fixture("tailwind_v4.css"))
    end

    test "reports malformed css but confirms it still round-trips" do
      assert {:error, %{valid: false, round_trips: true, diagnostics: n}} =
               IgniterCss.validate(".a { color: red;\n")

      assert n > 0
    end
  end

  describe "extract_*" do
    test "extracts colours by selector" do
      assert {:ok, [{".a", ["color: #333"]}, {".b", ["background: rgba(0,0,0,.5)"]}]} =
               IgniterCss.extract_colors(
                 ".a {\n  color: #333;\n  margin: 0;\n}\n.b {\n  background: rgba(0,0,0,.5);\n}\n"
               )
    end

    test "does not mistake a url path for a colour" do
      assert {:ok, []} = IgniterCss.extract_colors(".a { background: url(/red.png); }")
    end

    test "extracts media queries with their rules" do
      assert {:ok, [{"(max-width: 768px)", [{".a", [{"font-size", "14px"}]}]}]} =
               IgniterCss.extract_media_queries(
                 "@media (max-width: 768px) {\n  .a {\n    font-size: 14px;\n  }\n}\n"
               )
    end

    test "extracts animations and their users" do
      css = """
      @keyframes fade-in {
        from { opacity: 0; }
        to { opacity: 1; }
      }
      .a { animation: fade-in 1s; }
      """

      assert {:ok, [animation]} = IgniterCss.extract_animations(css)
      assert animation.name == "fade-in"
      assert animation.used_by == [".a"]
      assert animation.keyframes == [{"from", [{"opacity", "0"}]}, {"to", [{"opacity", "1"}]}]
    end
  end

  describe "refusing to patch" do
    test "unbalanced braces are rejected rather than half-edited" do
      for source <- [".broken {\n  color: red;\n", ".a {}\n}\n", "}"] do
        assert {:error, reason} = IgniterCss.ensure_rule(source, ".probe")
        assert reason =~ "unbalanced"
      end
    end

    test "analysis still works on a file we would refuse to patch" do
      assert {:ok, _} = IgniterCss.analyze(".broken {\n  color: red;\n")
      assert {:error, %{round_trips: true}} = IgniterCss.validate(".broken {\n  color: red;\n")
    end
  end

  describe "option handling" do
    test "line comments are tolerated by default" do
      assert {:ok, %{valid: true}} = IgniterCss.validate("// hi\n.a { color: red; }\n")
    end

    test "line comments can be rejected explicitly" do
      assert {:error, %{valid: false}} =
               IgniterCss.validate("// hi\n.a { color: red; }\n",
                 allow_wrong_line_comments: false
               )
    end
  end
end
