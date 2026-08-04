# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.TransformTest do
  use IgniterCss.CssCase, async: true

  doctest IgniterCss.Transform

  alias IgniterCss.Transform

  describe "minify/2" do
    test "keeps a space the grammar needs" do
      assert {:ok, "@media screen and (min-width:40em){.a{margin:1px -2px}}"} =
               Transform.minify(
                 "@media screen and (min-width: 40em) {\n  .a { margin: 1px -2px; }\n}\n"
               )
    end

    test "never adds a space before a function paren" do
      assert {:ok, ".a{background:url(a.png);transform:translate(1px)rotate(2deg)}"} =
               Transform.minify(
                 ".a {\n  background: url(a.png);\n  transform: translate(1px) rotate(2deg);\n}\n"
               )
    end

    test "preserves string contents exactly" do
      css = ~s|.a::after {\n  content: "a  b /* not a comment */ ;";\n}\n|
      assert {:ok, ~s|.a::after{content:"a  b /* not a comment */ ;"}|} = Transform.minify(css)
    end

    test "preserves non-ascii" do
      assert {:ok, ~s|.a::after{content:"日本語 ✓"}|} =
               Transform.minify(~s|.a::after {\n  content: "日本語 ✓";\n}\n|)
    end

    test "is idempotent" do
      {:ok, once} = Transform.minify(".a {\n  color: red;\n}\n")
      assert {:ok, ^once} = Transform.minify(once)
    end

    test "output still parses" do
      css = "@media print {\n  .a, .b > .c {\n    margin: 0 auto !important;\n  }\n}\n"
      assert {:ok, minified} = Transform.minify(css)
      assert {:ok, %{valid: true}} = IgniterCss.validate(minified)
    end

    test "never grows a file, across the whole corpus" do
      for {name, source} <- fixtures() do
        assert {:ok, minified} = Transform.minify(source)

        assert byte_size(minified) <= byte_size(source),
               "minifying grew #{name}"
      end
    end

    test "preserves a BOM" do
      assert {:ok, "﻿.a{color:red}"} = Transform.minify("﻿.a { color: red; }\n")
    end
  end

  describe "beautify/2" do
    test "indents nested blocks" do
      assert {:ok, "@media print {\n  .a {\n    color: red;\n  }\n}\n"} =
               Transform.beautify("@media print{.a{color:red}}")
    end

    test "keeps every comment, across the whole corpus" do
      for {name, source} <- fixtures() do
        assert {:ok, pretty} = Transform.beautify(source)

        missing = comments(source) -- comments(pretty)
        assert missing == [], "beautify lost #{inspect(missing)} from #{name}"
      end
    end

    test "does not comment out code after a line comment" do
      css = ".a {\n  padding: 0; // breathing room\n}\n.b { color: red; }\n"
      assert {:ok, pretty} = Transform.beautify(css)
      assert pretty =~ ".b {"
      assert {:ok, %{valid: true}} = IgniterCss.validate(pretty)
    end

    test "is idempotent" do
      {:ok, once} = Transform.beautify(".a{color:red;margin:0}@media print{.b{color:blue}}")
      assert {:ok, ^once} = Transform.beautify(once)
    end

    test "does not change what a stylesheet means" do
      for {name, source} <- fixtures() do
        assert {:ok, direct} = Transform.minify(source)
        assert {:ok, pretty} = Transform.beautify(source)
        assert {:ok, via_pretty} = Transform.minify(pretty)
        assert direct == via_pretty, "beautify changed the meaning of #{name}"
      end
    end

    test "an empty sheet stays empty" do
      assert {:ok, ""} = Transform.beautify("")
    end
  end

  describe "merge_stylesheets/2" do
    test "preserves comments from every sheet" do
      assert {:ok, merged} = Transform.merge_stylesheets(["/* one */\n.a {}", "/* two */\n.b {}"])
      assert merged =~ "/* one */"
      assert merged =~ "/* two */"
    end

    test "skips empty sheets" do
      assert {:ok, ".a {}\n"} = Transform.merge_stylesheets(["", ".a {}", "   "])
    end

    test "merged output still round-trips" do
      assert {:ok, merged} =
               Transform.merge_stylesheets([
                 fixture("phoenix_app.css"),
                 fixture("kitchen_sink.css")
               ])

      # Round-tripping is the property that matters: the Phoenix fixture carries
      # one parse diagnostic of its own (`@custom-variant ... *`), and merging
      # must not add any, but it does not have to remove it either.
      {_status, validation} = IgniterCss.validate(merged)
      assert validation.round_trips
      assert validation.diagnostics == 1
    end
  end
end
