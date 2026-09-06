# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.TransformTest do
  use IgniterCss.CssCase, async: true

  doctest IgniterCss.Transform

  alias IgniterCss.Transform

  @complex """
  /* header */
  @layer base, components;

  @import "./typography.css" layer(components);

  @theme {
    --font-display: "Satoshi", sans-serif;
    --ease-fluid: cubic-bezier(0.3, 0, 0, 1);
  }

  a:hover,
  a:focus-visible::before {
    content: "a:b";
  }

  .card:not([data-open]) .body,
  .card:not(:hover, :focus-within) > .footer {
    display: none;
  }

  li:nth-child(2n + 1):not(:last-child) {
    background: url(data:image/svg+xml;base64,PHN2Zy8+) no-repeat;
  }

  .grid {
    grid-template-areas:
      "head head"
      "side main";
    grid-template-columns: minmax(12rem, 1fr) 3fr;
  }

  @media (min-width: 40rem) and (max-width: 80rem) {
    .x::selection {
      color: white !important;
    }
  }

  @supports (display: grid) and (not (display: inline-grid)) {
    .y {
      --shadow: 0 1px 2px rgb(0 0 0 / 0.05);
    }
  }

  .typography {
    h1 {
      @apply text-2xl;
    }
  }
  """

  describe "beautify/2 on complex CSS" do
    test "keeps every pseudo-class and pseudo-element attached to its selector" do
      assert {:ok, out} = Transform.beautify(@complex)

      for selector <- [
            "a:hover",
            "a:focus-visible::before",
            ".card:not([data-open])",
            ":not(:hover, :focus-within)",
            "li:nth-child(2n + 1):not(:last-child)",
            ".x::selection"
          ] do
        assert String.contains?(out, selector), "lost #{selector}"
      end

      refute out =~ ~r/:\s+(not|hover|nth-child|selection|focus-visible)\b/
    end

    test "keeps the separators a value needs" do
      assert {:ok, out} = Transform.beautify(@complex)

      assert out =~ "url(data:image/svg+xml;base64,PHN2Zy8+) no-repeat"
      assert out =~ "minmax(12rem, 1fr) 3fr"
      assert out =~ ~s|"head head" "side main"|
      assert out =~ "rgb(0 0 0 / 0.05)"
      assert out =~ "cubic-bezier(0.3, 0, 0, 1)"
    end

    test "keeps Tailwind's own at-rules intact" do
      assert {:ok, out} = Transform.beautify(@complex)

      assert {:ok, [theme]} = IgniterCss.get_at_rules(out, "theme")
      assert length(theme.declarations) == 2
      assert out =~ "@layer base, components;"
      assert out =~ ~s|@import "./typography.css" layer(components);|
      assert out =~ "@apply text-2xl"
    end

    test "preserves every selector and declaration" do
      assert {:ok, out} = Transform.beautify(@complex)
      assert {:ok, before} = IgniterCss.list_selectors(@complex)
      assert {:ok, after_} = IgniterCss.list_selectors(out)

      squeeze = fn list ->
        list |> Enum.map(&(&1 |> String.split() |> Enum.join(" "))) |> Enum.sort()
      end

      assert squeeze.(before) == squeeze.(after_)

      collapse = fn {:ok, ds} ->
        Enum.map(ds, fn {k, v} -> {k, v |> String.split() |> Enum.join(" ")} end)
      end

      assert collapse.(IgniterCss.get_rule_declarations(@complex, ".grid")) ==
               collapse.(IgniterCss.get_rule_declarations(out, ".grid"))
    end

    test "is idempotent and still parses" do
      assert {:ok, once} = Transform.beautify(@complex)
      assert {:ok, twice} = Transform.beautify(once)
      assert once == twice
      assert {:ok, _} = IgniterCss.validate(once)
    end

    test "keeps every comment" do
      assert {:ok, out} = Transform.beautify(@complex)
      assert out =~ "/* header */"
    end

    test "survives a minify then beautify round trip" do
      assert {:ok, small} = Transform.minify(@complex)
      assert {:ok, pretty} = Transform.beautify(small)

      assert {:ok, before} = IgniterCss.list_selectors(@complex)
      assert {:ok, after_} = IgniterCss.list_selectors(pretty)
      assert length(before) == length(after_)
      assert pretty =~ ".typography"
      assert pretty =~ "@apply text-2xl"
    end
  end

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
