# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCssTest.Parsers.Css.ParserTest do
  @moduledoc """
  The compatibility surface: same function names and the same
  `{:ok, :function_name, result}` shape the Python implementation used, now
  backed by the Rust parser.
  """

  use IgniterCss.CssCase, async: true

  doctest IgniterCss.Parsers.Parser

  alias IgniterCss.Parsers.Parser

  describe "add_hide_scrollbar_property/2" do
    test "adds display: none to an existing .hide-scrollbar class" do
      css = """
      .header {
        color: blue;
      }

      .hide-scrollbar {
        scrollbar-width: none; /* Firefox */
      }
      """

      assert {:ok, :add_hide_scrollbar_property, result} =
               Parser.add_hide_scrollbar_property(css)

      assert result == """
             .header {
               color: blue;
             }

             .hide-scrollbar {
               scrollbar-width: none; /* Firefox */
               display: none;
             }
             """

      assert_comments_preserved(css, result)
    end

    test "creates the class when it does not exist" do
      css = ".header {\n  color: blue;\n}\n"
      assert {:ok, _, result} = Parser.add_hide_scrollbar_property(css)
      assert result == ".header {\n  color: blue;\n}\n\n.hide-scrollbar {\n  display: none;\n}\n"
    end

    test "updates an existing display property" do
      css = ".hide-scrollbar {\n  display: block;\n}\n"
      assert {:ok, _, result} = Parser.add_hide_scrollbar_property(css)
      assert result == ".hide-scrollbar {\n  display: none;\n}\n"
    end

    test "works with empty CSS" do
      assert {:ok, _, ".hide-scrollbar {\n  display: none;\n}\n"} =
               Parser.add_hide_scrollbar_property("")
    end

    test "is idempotent" do
      css = ".hide-scrollbar {\n  scrollbar-width: none;\n}\n"
      assert {:ok, _, once} = Parser.add_hide_scrollbar_property(css)
      assert {:ok, _, twice} = Parser.add_hide_scrollbar_property(once)
      assert once == twice
    end

    test "reads from a file path" do
      path = Path.join(System.tmp_dir!(), "igniter_css_#{System.unique_integer([:positive])}.css")
      File.write!(path, ".hide-scrollbar {\n  scrollbar-width: none;\n}\n")

      try do
        assert {:ok, _, result} = Parser.add_hide_scrollbar_property(path, :path)
        assert result =~ "display: none;"
      after
        File.rm(path)
      end
    end

    test "rejects a path that is not a stylesheet" do
      assert {:error, _, "Invalid file path or format."} =
               Parser.add_hide_scrollbar_property("/nope/nothing.txt", :path)
    end
  end

  describe "add_vendor_prefixes/4" do
    test "adds prefixes to an existing property" do
      css = ".a {\n  user-select: none;\n}\n"

      assert {:ok, _, result} =
               Parser.add_vendor_prefixes(css, "user-select", ["-webkit-", "-ms-"])

      assert result ==
               ".a {\n  -webkit-user-select: none;\n  -ms-user-select: none;\n  user-select: none;\n}\n"
    end

    test "does nothing when the property is absent" do
      css = ".a {\n  color: red;\n}\n"
      assert {:ok, _, ^css} = Parser.add_vendor_prefixes(css, "user-select", ["-webkit-"])
    end

    test "handles every occurrence, including inside media queries" do
      css = ".a { user-select: none; }\n@media print {\n  .b {\n    user-select: text;\n  }\n}\n"
      assert {:ok, _, result} = Parser.add_vendor_prefixes(css, "user-select", ["-webkit-"])
      assert result =~ "-webkit-user-select: none;"
      assert result =~ "-webkit-user-select: text;"
    end

    test "works with an empty prefix list" do
      css = ".a {\n  user-select: none;\n}\n"
      assert {:ok, _, ^css} = Parser.add_vendor_prefixes(css, "user-select", [])
    end

    test "preserves !important flags" do
      css = ".a {\n  user-select: none !important;\n}\n"
      assert {:ok, _, result} = Parser.add_vendor_prefixes(css, "user-select", ["-webkit-"])
      assert result =~ "-webkit-user-select: none !important;"
      assert result =~ "user-select: none !important;"
    end

    test "preserves comments" do
      css = ".a {\n  /* no selection */\n  user-select: none; /* anywhere */\n}\n"
      assert {:ok, _, result} = Parser.add_vendor_prefixes(css, "user-select", ["-webkit-"])
      assert_comments_preserved(css, result)
    end

    test "refuses invalid CSS rather than half-editing it" do
      assert {:error, _, reason} =
               Parser.add_vendor_prefixes(".a { user-select: none;", "user-select", ["-webkit-"])

      assert reason =~ "unbalanced"
    end
  end

  describe "modify_property/6" do
    test "changes a property value" do
      assert {:ok, :modify_property, ".a { color: blue; }"} =
               Parser.modify_property(".a { color: red; }", ".a", "color", "blue", false)
    end

    test "marks a property important" do
      assert {:ok, _, ".a { color: blue !important; }"} =
               Parser.modify_property(".a { color: red; }", ".a", "color", "blue", true)
    end

    test "adds the property when the rule lacks it" do
      assert {:ok, _, ".a {\n  color: red;\n  margin: 0;\n}\n"} =
               Parser.modify_property(".a {\n  color: red;\n}\n", ".a", "margin", "0", false)
    end

    test "creates the rule when the selector is absent" do
      assert {:ok, _, ".a {}\n\n.b {\n  color: red;\n}\n"} =
               Parser.modify_property(".a {}\n", ".b", "color", "red", false)
    end

    test "preserves a trailing comment on the modified line" do
      css = ".a {\n  color: red; /* brand */\n}\n"
      assert {:ok, _, result} = Parser.modify_property(css, ".a", "color", "blue", false)
      assert result == ".a {\n  color: blue; /* brand */\n}\n"
    end

    test "refuses an ambiguous selector" do
      assert {:error, _, reason} =
               Parser.modify_property(".a {}\n.a {}\n", ".a", "color", "red", false)

      assert reason =~ "refusing to guess"
    end
  end

  describe "remove_selector/3" do
    test "removes a selector and its block" do
      assert {:ok, :remove_selector, ".a {}\n"} =
               Parser.remove_selector(".a {}\n.unused {\n  color: red;\n}\n", ".unused")
    end

    test "leaves other selectors alone" do
      css = ".a { color: red; }\n.b { color: blue; }\n.c { color: green; }\n"
      assert {:ok, _, result} = Parser.remove_selector(css, ".b")
      assert result == ".a { color: red; }\n.c { color: green; }\n"
    end

    test "removing an absent selector changes nothing" do
      css = ".a {}\n"
      assert {:ok, _, ^css} = Parser.remove_selector(css, ".zz")
    end

    test "keeps a section header above the removed rule" do
      css = "/* ===== Utils ===== */\n.b {}\n.c {}\n"
      assert {:ok, _, "/* ===== Utils ===== */\n.c {}\n"} = Parser.remove_selector(css, ".b")
    end
  end

  describe "replace_selector_rule/4" do
    test "replaces the declarations of a rule" do
      assert {:ok, :replace_selector_rule, ".a { color: blue; font-size: 20px; }"} =
               Parser.replace_selector_rule(
                 ".a { color: red; }",
                 ".a",
                 "color: blue; font-size: 20px;"
               )
    end

    test "leaves the rest of the file untouched" do
      css = "/* head */\n.a {\n  color: red;\n}\n/* tail */\n.b {}\n"
      assert {:ok, _, result} = Parser.replace_selector_rule(css, ".a", "color: blue")
      assert result == "/* head */\n.a {\n  color: blue;\n}\n/* tail */\n.b {}\n"
      assert_comments_preserved(css, result)
    end

    test "errors on a missing selector" do
      assert {:error, _, reason} = Parser.replace_selector_rule(".a {}", ".zz", "color: red")
      assert reason =~ "not found"
    end
  end

  describe "add_import/4 and remove_import/3" do
    test "adds an import with no media query" do
      assert {:ok, :add_import, ~s|@import "styles.css";\n|} =
               Parser.add_import("", "styles.css", false)
    end

    test "adds an import with a media query" do
      assert {:ok, _, ~s|@import "mobile.css" screen and (max-width: 768px);\n|} =
               Parser.add_import("", "mobile.css", "screen and (max-width: 768px)")
    end

    test "does not add a duplicate import" do
      css = ~s|@import "styles.css";\n|
      assert {:ok, _, ^css} = Parser.add_import(css, "styles.css", false)
    end

    test "places the import before existing rules" do
      assert {:ok, _, result} = Parser.add_import(".a { color: red; }\n", "x.css", false)
      assert result == ~s|@import "x.css";\n.a { color: red; }\n|
    end

    test "removes a matching import" do
      css = ~s|@import "a.css";\n@import "b.css";\n.x {}\n|

      assert {:ok, :remove_import, ~s|@import "b.css";\n.x {}\n|} =
               Parser.remove_import(css, "a.css")
    end

    test "removing an absent import changes nothing" do
      css = ~s|@import "b.css";\n|
      assert {:ok, _, ^css} = Parser.remove_import(css, "a.css")
    end
  end

  describe "sort_properties/2" do
    test "sorts properties alphabetically" do
      css = ".a {\n  font-size: 16px;\n  background: #fff;\n  color: #333;\n}\n"
      assert {:ok, :sort_properties, result} = Parser.sort_properties(css)
      assert result == ".a {\n  background: #fff;\n  color: #333;\n  font-size: 16px;\n}\n"
    end

    test "leaves an already sorted sheet alone" do
      css = ".a {\n  background: #fff;\n  color: #333;\n}\n"
      assert {:ok, _, ^css} = Parser.sort_properties(css)
    end

    test "preserves comments" do
      css = ".a {\n  /* z */\n  z-index: 1;\n  color: red;\n}\n"
      assert {:ok, _, result} = Parser.sort_properties(css)
      assert_comments_preserved(css, result)
    end
  end

  describe "remove_duplicates/2" do
    test "drops declarations a later one shadows" do
      assert {:ok, :remove_duplicates, ".a {\n  color: blue;\n}\n"} =
               Parser.remove_duplicates(".a {\n  color: red;\n  color: blue;\n}\n")
    end

    test "drops an identical duplicated rule" do
      assert {:ok, _, ".b {}\n\n.a { color: red; }\n"} =
               Parser.remove_duplicates(".a { color: red; }\n\n.b {}\n\n.a { color: red; }\n")
    end

    test "keeps rules that differ" do
      css = ".a { color: red; }\n.a { margin: 0; }\n"
      assert {:ok, _, ^css} = Parser.remove_duplicates(css)
    end
  end

  describe "minify/2 and beautify/2" do
    test "minifies a stylesheet" do
      css = ".header {\n  color: #333;\n  background: #fff;\n}\n\n.footer {\n  color: #000;\n}\n"

      assert {:ok, :minify, ".header{color:#333;background:#fff}.footer{color:#000}"} =
               Parser.minify(css)
    end

    test "minifying removes comments" do
      assert {:ok, _, ".a{color:red}"} = Parser.minify("/* c */\n.a { color: red; /* d */ }\n")
    end

    test "beautifies a minified stylesheet" do
      assert {:ok, :beautify, ".a {\n  color: red;\n  background: #fff;\n}\n"} =
               Parser.beautify(".a{color:red;background:#fff}")
    end

    test "beautifying keeps comments" do
      css = "/* head */.a{color:red}"
      assert {:ok, _, result} = Parser.beautify(css)
      assert_comments_preserved(css, result)
    end

    test "minify and beautify are inverse enough to round-trip meaning" do
      css = ".a{color:red;margin:0 auto}.b,.c>.d{padding:0}"
      assert {:ok, _, pretty} = Parser.beautify(css)
      assert {:ok, _, back} = Parser.minify(pretty)
      assert back == css
    end
  end

  describe "merge_stylesheets/1" do
    test "merges two stylesheets" do
      assert {:ok, :merge_stylesheets, ".a { color: red; }\n\n.b { color: blue; }\n"} =
               Parser.merge_stylesheets([".a { color: red; }", ".b { color: blue; }"])
    end

    test "drops an identical repeated rule" do
      assert {:ok, _, ".a { color: red; }\n"} =
               Parser.merge_stylesheets([".a { color: red; }", ".a { color: red; }"])
    end

    test "keeps a later rule that overrides" do
      assert {:ok, _, result} =
               Parser.merge_stylesheets([".a { color: red; }", ".a { color: blue; }"])

      assert result =~ "color: red"
      assert result =~ "color: blue"
    end

    test "merging nothing gives an empty string" do
      assert {:ok, _, ""} = Parser.merge_stylesheets([])
    end
  end

  describe "analyze_css/2" do
    test "returns analysis for a multi-selector stylesheet" do
      css = """
      .header {
        color: #333;
        background: #fff;
      }

      .footer {
        color: #000;
      }
      """

      assert {:ok, :analyze_css, stats} = Parser.analyze_css(css)
      assert stats["rules_count"] == 2
      assert stats["declarations_count"] == 3
      # `color` twice plus `background` once.
      assert stats["unique_properties"] == 2
      assert stats["colors_count"] == 3
      assert stats["property_frequency"]["color"] == 2
    end

    test "counts media queries and imports" do
      css = ~s|@import "a";\n@media print {\n  .a { color: red; }\n}\n|
      assert {:ok, _, stats} = Parser.analyze_css(css)
      assert stats["imports_count"] == 1
      assert stats["media_queries_count"] == 1
    end

    test "analyses an empty stylesheet" do
      assert {:ok, _, stats} = Parser.analyze_css("")
      assert stats["rules_count"] == 0
      assert stats["declarations_count"] == 0
    end

    test "counts comments" do
      assert {:ok, _, stats} = Parser.analyze_css("/* a */\n.x { /* b */ color: red; }\n")
      assert stats["comments_count"] == 2
    end
  end

  describe "extract_colors/2" do
    test "groups colours by selector" do
      css =
        ".header {\n  color: #333;\n  background-color: white;\n}\n.footer {\n  color: rgba(0, 0, 0, 0.8);\n}\n"

      assert {:ok, :extract_colors, colors} = Parser.extract_colors(css)

      assert colors == %{
               ".header" => ["color: #333", "background-color: white"],
               ".footer" => ["color: rgba(0, 0, 0, 0.8)"]
             }
    end

    test "ignores declarations that carry no colour" do
      assert {:ok, _, %{}} = Parser.extract_colors(".a { margin: 0; display: flex; }")
    end
  end

  describe "extract_media_queries/2" do
    test "returns rules keyed by query" do
      css = "@media (max-width: 768px) {\n  .header {\n    font-size: 14px;\n  }\n}\n"

      assert {:ok, :extract_media_queries, queries} = Parser.extract_media_queries(css)

      assert queries == %{
               "(max-width: 768px)" => [
                 %{"selector" => ".header", "properties" => %{"font-size" => "14px"}}
               ]
             }
    end

    test "a stylesheet without media queries yields an empty map" do
      assert {:ok, _, %{}} = Parser.extract_media_queries(".a {}\n")
    end
  end

  describe "extract_animations/2" do
    test "returns keyframes and users" do
      css = """
      @keyframes fade-in {
        0% { opacity: 0; }
        100% { opacity: 1; }
      }

      .header { animation: fade-in 1s; }
      .modal { animation-name: fade-in; }
      """

      assert {:ok, :extract_animations, animations} = Parser.extract_animations(css)

      assert animations == %{
               "fade-in" => %{
                 "keyframes" => %{"0%" => %{"opacity" => "0"}, "100%" => %{"opacity" => "1"}},
                 "used_by" => [".header", ".modal"]
               }
             }
    end

    test "an unused animation lists no users" do
      assert {:ok, _, %{"x" => %{"used_by" => []}}} =
               Parser.extract_animations("@keyframes x {\n  0% { left: 0; }\n}\n")
    end
  end

  describe "validate_css/2" do
    test "accepts valid CSS" do
      assert {:ok, :validate_css, true} = Parser.validate_css(".a { color: red; }")
    end

    test "accepts Tailwind v4 syntax" do
      assert {:ok, _, true} =
               Parser.validate_css(~s|@import "tailwindcss";\n@theme {\n  --c: red;\n}\n|)
    end

    test "rejects malformed CSS with a message" do
      assert {:error, :validate_css, message} = Parser.validate_css("invalid { css")
      assert is_binary(message)
      assert message =~ "diagnostic"
    end
  end

  describe "selector_exists?/3" do
    test "finds an existing selector" do
      assert {:ok, :selector_exists?, true} = Parser.selector_exists?(".header {}", ".header")
    end

    test "reports a missing selector" do
      assert {:error, :selector_exists?, false} = Parser.selector_exists?(".header {}", "#nope")
    end

    test "normalises whitespace before comparing" do
      assert {:ok, _, true} = Parser.selector_exists?(".a  >  .b {}", ".a>.b")
    end
  end

  describe "get_selector_properties/3" do
    test "returns a property map" do
      assert {:ok, :get_selector_properties, %{"color" => "blue", "font-size" => "16px"}} =
               Parser.get_selector_properties(".a { color: blue; font-size: 16px; }", ".a")
    end

    test "returns nil for a missing selector" do
      assert {:ok, _, nil} = Parser.get_selector_properties(".a {}", "#nope")
    end

    test "includes the !important flag in the value" do
      assert {:ok, _, %{"color" => "red !important"}} =
               Parser.get_selector_properties(".a { color: red !important; }", ".a")
    end
  end
end
