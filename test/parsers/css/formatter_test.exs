# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCssTest.Parsers.Css.FormatterTest do
  use IgniterCss.CssCase, async: true

  doctest IgniterCss.Parsers.Formatter

  alias IgniterCss.Parsers.Formatter

  test "formats a minified stylesheet" do
    assert {:ok, :format, ".a {\n  color: red;\n  margin: 0;\n}\n"} =
             Formatter.format(".a{color:red;margin:0}")
  end

  test "formatting keeps comments" do
    css = "/* head */.a{color:red}"
    assert {:ok, _, formatted} = Formatter.format(css)
    assert_comments_preserved(css, formatted)
  end

  test "reports an already formatted stylesheet" do
    assert {:ok, :is_formatted, true} = Formatter.is_formatted(".a {\n  color: red;\n}\n")
  end

  test "reports an unformatted stylesheet" do
    assert {:error, :is_formatted, false} = Formatter.is_formatted(".a{color:red}")
  end

  test "formatting is idempotent" do
    assert {:ok, _, once} = Formatter.format(".a{color:red}@media print{.b{margin:0}}")
    assert {:ok, _, ^once} = Formatter.format(once)
    assert {:ok, _, true} = Formatter.is_formatted(once)
  end

  test "formats from a file path" do
    path =
      Path.join(System.tmp_dir!(), "igniter_css_fmt_#{System.unique_integer([:positive])}.css")

    File.write!(path, ".a{color:red}")

    try do
      assert {:ok, _, ".a {\n  color: red;\n}\n"} = Formatter.format(path, :path)
    after
      File.rm(path)
    end
  end
end
