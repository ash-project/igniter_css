# SPDX-FileCopyrightText: 2025 Shahryar Tavakkoli
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.Parsers.Formatter do
  @moduledoc """
  Provides formatting functionality that requires igniter_js.
  This module's functions will only work when igniter_js is included as a dependency.
  """

  if Code.ensure_loaded?(IgniterJs) do
    alias IgniterJs.Parsers.CSS.Formatter
    defdelegate format(file_path_or_content, type \\ :content), to: Formatter
    defdelegate is_formatted(file_path_or_content, type \\ :content), to: Formatter
  else
    def format(_) do
      {:error, :igniter_js_not_available}
    end
  end
end
