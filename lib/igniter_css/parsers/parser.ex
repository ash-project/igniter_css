# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.Parsers.Parser do
  @moduledoc """
  The full CSS toolkit surface, on the `{:ok, :function_name, result}` calling
  convention shared with `igniter_js`.

  Every function accepts either CSS content or a file path, selected by the
  trailing `type` argument (`:content`, the default, or `:path`).

  This module covers the same ground the previous Python/tinycss2 implementation
  did, reimplemented on the Rust parser. Two differences are worth knowing:

  * The mutating functions here are now **diff-minimal** — they patch byte
    ranges instead of reprinting the stylesheet, so comments and formatting
    outside the edit are preserved exactly.
  * `minify/2` and `beautify/2` still rewrite the whole file, because that is
    what they are for. Do not use them to patch a file a user maintains.

  For new code prefer `IgniterCss`, which has a plainer `{:ok, result}` shape
  and clearer option handling. This module exists so existing call sites keep
  working.
  """

  import IgniterCss.Helpers, only: [call_nif_fn: 4]

  alias IgniterCss.{Native, ParseOpts, Transform}

  @type type :: :content | :path

  # ---------------------------------------------------------------------------
  # Mutating operations
  # ---------------------------------------------------------------------------

  @doc """
  Add `display: none` to `.hide-scrollbar`, creating the class if it is absent.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.add_hide_scrollbar_property("")
      iex> css
      ".hide-scrollbar {\\n  display: none;\\n}\\n"
  """
  @spec add_hide_scrollbar_property(String.t(), type()) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def add_hide_scrollbar_property(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        Native.set_declaration_nif(
          content,
          ".hide-scrollbar",
          "display",
          "none",
          nil,
          true,
          ParseOpts.new([])
        )
        |> to_source()
      end,
      type
    )
  end

  @doc """
  Add vendor-prefixed copies of `property_name` beside every occurrence of it.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.add_vendor_prefixes(
      ...>   ".a { user-select: none; }", "user-select", ["-webkit-"])
      iex> css
      ".a { -webkit-user-select: none; user-select: none; }"
  """
  @spec add_vendor_prefixes(String.t(), String.t(), [String.t()], type()) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def add_vendor_prefixes(file_path_or_content, property_name, prefixes, type \\ :content)
      when is_list(prefixes) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        Native.add_vendor_prefixes_nif(content, property_name, prefixes, ParseOpts.new([]))
        |> to_source()
      end,
      type
    )
  end

  @doc """
  Set a property value on a selector.

  Only the value bytes change when the property already exists, so an inline
  comment on that line survives.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.modify_property(
      ...>   ".a { color: red; }", ".a", "color", "blue", false)
      iex> css
      ".a { color: blue; }"
  """
  @spec modify_property(String.t(), String.t(), String.t(), String.t(), boolean(), type()) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def modify_property(
        file_path_or_content,
        selector,
        property_name,
        new_value,
        important \\ false,
        type \\ :content
      )
      when is_boolean(important) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        Native.set_declaration_nif(
          content,
          selector,
          property_name,
          new_value,
          important,
          true,
          ParseOpts.new([])
        )
        |> to_source()
      end,
      type
    )
  end

  @doc """
  Remove a top-level selector and everything it owns.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.remove_selector(
      ...>   ".a {}\\n.unused {}\\n", ".unused")
      iex> css
      ".a {}\\n"
  """
  @spec remove_selector(String.t(), String.t(), type()) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def remove_selector(file_path_or_content, selector, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        Native.remove_rule_nif(content, selector, ParseOpts.new([])) |> to_source()
      end,
      type
    )
  end

  @doc """
  Replace a rule's declarations wholesale.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.replace_selector_rule(
      ...>   ".a { color: red; }", ".a", "color: blue; padding: 10px;")
      iex> css
      ".a { color: blue; padding: 10px; }"
  """
  @spec replace_selector_rule(String.t(), String.t(), String.t(), type()) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def replace_selector_rule(file_path_or_content, selector, new_declarations, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        Native.replace_rule_body_nif(content, selector, new_declarations, ParseOpts.new([]))
        |> to_source()
      end,
      type
    )
  end

  @doc """
  Add an `@import` unless an equivalent one is already present.

  `media_query` may be a media query string, or `false`/`nil` for none.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.add_import("", "styles.css", false)
      iex> css
      ~s|@import "styles.css";\\n|
  """
  @spec add_import(String.t(), String.t(), String.t() | boolean() | nil, type()) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def add_import(file_path_or_content, import_url, media_query \\ nil, type \\ :content)
      when is_boolean(media_query) or is_binary(media_query) or is_nil(media_query) do
    media = if is_binary(media_query) and media_query != "", do: media_query

    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        Native.add_import_nif(content, import_url, media, ParseOpts.new([])) |> to_source()
      end,
      type
    )
  end

  @doc """
  Remove `@import` rules pointing at `import_url`.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.remove_import(
      ...>   ~s|@import "a.css";\\n@import "b.css";\\n|, "a.css")
      iex> css
      ~s|@import "b.css";\\n|
  """
  @spec remove_import(String.t(), String.t(), type()) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def remove_import(file_path_or_content, import_url, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        Native.remove_import_nif(content, import_url, ParseOpts.new([])) |> to_source()
      end,
      type
    )
  end

  @doc """
  Sort declarations alphabetically within each block, by moving whole lines.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.sort_properties(
      ...>   ".a {\\n  color: red;\\n  background: #fff;\\n}\\n")
      iex> css
      ".a {\\n  background: #fff;\\n  color: red;\\n}\\n"
  """
  @spec sort_properties(String.t(), type()) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def sort_properties(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        Native.sort_properties_nif(content, ParseOpts.new([])) |> to_source()
      end,
      type
    )
  end

  @doc """
  Remove declarations and rules that a later one makes redundant.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.remove_duplicates(
      ...>   ".a {\\n  color: red;\\n  color: blue;\\n}\\n")
      iex> css
      ".a {\\n  color: blue;\\n}\\n"
  """
  @spec remove_duplicates(String.t(), type()) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def remove_duplicates(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        Native.remove_duplicates_nif(content, true, true, ParseOpts.new([])) |> to_source()
      end,
      type
    )
  end

  # ---------------------------------------------------------------------------
  # Whole-file transforms
  # ---------------------------------------------------------------------------

  @doc """
  Minify a stylesheet. Rewrites the whole file and drops comments by design.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.minify(".a {\\n  color: red;\\n}\\n")
      iex> css
      ".a{color:red}"
  """
  @spec minify(String.t(), type()) :: {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def minify(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content -> Native.minify_nif(content, ParseOpts.new([])) end,
      type
    )
  end

  @doc """
  Pretty-print a stylesheet, keeping every comment. Rewrites the whole file.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.beautify(".a{color:red}")
      iex> css
      ".a {\\n  color: red;\\n}\\n"
  """
  @spec beautify(String.t(), type()) :: {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def beautify(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content -> Native.beautify_nif(content, ParseOpts.new([])) end,
      type
    )
  end

  @doc """
  Concatenate stylesheets, dropping rules a later identical copy makes
  redundant.

  ## Examples

      iex> {:ok, _, css} = IgniterCss.Parsers.Parser.merge_stylesheets([".a {}", ".b {}"])
      iex> css
      ".a {}\\n\\n.b {}\\n"
  """
  @spec merge_stylesheets([String.t()]) ::
          {:ok, atom(), String.t()} | {:error, atom(), String.t()}
  def merge_stylesheets(css_list) when is_list(css_list) do
    case Transform.merge_stylesheets(css_list) do
      {:ok, merged} -> {:ok, :merge_stylesheets, merged}
      {:error, reason} -> {:error, :merge_stylesheets, reason}
    end
  end

  # ---------------------------------------------------------------------------
  # Analysis
  # ---------------------------------------------------------------------------

  @doc """
  Statistics about a stylesheet, as a string-keyed map.

  ## Examples

      iex> {:ok, _, stats} = IgniterCss.Parsers.Parser.analyze_css(".a { color: red; }")
      iex> {stats["rules_count"], stats["declarations_count"]}
      {1, 1}
  """
  @spec analyze_css(String.t(), type()) :: {:ok, atom(), map()} | {:error, atom(), String.t()}
  def analyze_css(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        case Native.analyze_nif(content, ParseOpts.new([])) do
          {:ok, fun, analysis} ->
            map =
              analysis
              |> Map.from_struct()
              |> Map.new(fn
                {:property_frequency, pairs} ->
                  {"property_frequency", Map.new(pairs, fn {k, v} -> {k, v} end)}

                {key, value} ->
                  {Atom.to_string(key), value}
              end)

            {:ok, fun, map}

          other ->
            other
        end
      end,
      type
    )
  end

  @doc """
  Colour-carrying declarations, keyed by selector.

  ## Examples

      iex> {:ok, _, colors} = IgniterCss.Parsers.Parser.extract_colors(".a { color: #333; }")
      iex> colors
      %{".a" => ["color: #333"]}
  """
  @spec extract_colors(String.t(), type()) :: {:ok, atom(), map()} | {:error, atom(), String.t()}
  def extract_colors(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        case Native.extract_colors_nif(content, ParseOpts.new([])) do
          {:ok, fun, pairs} -> {:ok, fun, Map.new(pairs)}
          other -> other
        end
      end,
      type
    )
  end

  @doc """
  Media queries keyed by condition, each holding a list of
  `%{"selector" => ..., "properties" => %{...}}`.

  ## Examples

      iex> css = "@media print {\\n  .a { display: none; }\\n}\\n"
      iex> {:ok, _, queries} = IgniterCss.Parsers.Parser.extract_media_queries(css)
      iex> queries
      %{"print" => [%{"selector" => ".a", "properties" => %{"display" => "none"}}]}
  """
  @spec extract_media_queries(String.t(), type()) ::
          {:ok, atom(), map()} | {:error, atom(), String.t()}
  def extract_media_queries(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        case Native.extract_media_queries_nif(content, ParseOpts.new([])) do
          {:ok, fun, queries} ->
            map =
              Map.new(queries, fn {query, rules} ->
                {query,
                 Enum.map(rules, fn {selector, declarations} ->
                   %{"selector" => selector, "properties" => Map.new(declarations)}
                 end)}
              end)

            {:ok, fun, map}

          other ->
            other
        end
      end,
      type
    )
  end

  @doc """
  Animations keyed by name, each holding `"keyframes"` and `"used_by"`.

  ## Examples

      iex> css = "@keyframes fade {\\n  from { opacity: 0; }\\n}\\n.a { animation: fade 1s; }\\n"
      iex> {:ok, _, animations} = IgniterCss.Parsers.Parser.extract_animations(css)
      iex> animations["fade"]["used_by"]
      [".a"]
  """
  @spec extract_animations(String.t(), type()) ::
          {:ok, atom(), map()} | {:error, atom(), String.t()}
  def extract_animations(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        case Native.extract_animations_nif(content, ParseOpts.new([])) do
          {:ok, fun, animations} ->
            map =
              Map.new(animations, fn animation ->
                {animation.name,
                 %{
                   "keyframes" =>
                     Map.new(animation.keyframes, fn {step, declarations} ->
                       {step, Map.new(declarations)}
                     end),
                   "used_by" => animation.used_by
                 }}
              end)

            {:ok, fun, map}

          other ->
            other
        end
      end,
      type
    )
  end

  @doc """
  Is this CSS understood well enough to patch?

  Returns `{:ok, :validate_css, true}` when it is, and
  `{:error, :validate_css, message}` when it is not.

  ## Examples

      iex> IgniterCss.Parsers.Parser.validate_css(".a { color: red; }")
      {:ok, :validate_css, true}
  """
  @spec validate_css(String.t(), type()) ::
          {:ok, atom(), true} | {:error, atom(), String.t()}
  def validate_css(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        case Native.validate_nif(content, ParseOpts.new([])) do
          {:ok, fun, _validation} -> {:ok, fun, true}
          {:error, fun, validation} -> {:error, fun, validation.message}
        end
      end,
      type
    )
  end

  @doc """
  Does a top-level rule with this selector exist?

  ## Examples

      iex> IgniterCss.Parsers.Parser.selector_exists?(".a { color: red; }", ".a")
      {:ok, :selector_exists?, true}

      iex> IgniterCss.Parsers.Parser.selector_exists?(".a { color: red; }", "#nope")
      {:error, :selector_exists?, false}
  """
  @spec selector_exists?(String.t(), String.t(), type()) ::
          {:ok, atom(), true} | {:error, atom(), false}
  def selector_exists?(file_path_or_content, selector, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        case Native.has_rule_nif(content, selector, ParseOpts.new([])) do
          {:ok, fun, true} -> {:ok, fun, true}
          {_status, fun, _} -> {:error, fun, false}
        end
      end,
      type
    )
  end

  @doc """
  The declarations of a selector as a `%{property => value}` map, or `nil` when
  the selector is absent.

  ## Examples

      iex> {:ok, _, props} = IgniterCss.Parsers.Parser.get_selector_properties(
      ...>   ".a { color: blue; font-size: 16px; }", ".a")
      iex> props
      %{"color" => "blue", "font-size" => "16px"}
  """
  @spec get_selector_properties(String.t(), String.t(), type()) ::
          {:ok, atom(), map() | nil} | {:error, atom(), String.t()}
  def get_selector_properties(file_path_or_content, selector, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn content ->
        case Native.get_rule_declarations_nif(content, selector, ParseOpts.new([])) do
          {:ok, fun, nil} -> {:ok, fun, nil}
          {:ok, fun, declarations} -> {:ok, fun, Map.new(declarations)}
          other -> other
        end
      end,
      type
    )
  end

  # ---------------------------------------------------------------------------

  # The native layer returns a rich outcome; this convention only carries the
  # patched source.
  defp to_source({:ok, fun, outcome}), do: {:ok, fun, outcome.source}
  defp to_source({:error, fun, reason}), do: {:error, fun, reason}
end
