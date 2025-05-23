defmodule IgniterCss.Parsers.Parser do
  @moduledoc """
  CSS parsing and manipulation using Python's tinycss2 library.

  This module provides functions to work with CSS files by leveraging
  a Python toolkit built on tinycss2 for parsing, modifying, and analyzing CSS.

  > **Please note that the use of Python in Elixir will remain experimental for now,
  > as we continue to improve it over time and decide whether to adopt it fully.**
  """

  import IgniterCss.Helpers, only: [call_nif_fn: 4]

  @doc """
  Adds a display: none property to the .hide-scrollbar class.
  If the class doesn't exist, it creates it.

  ## Examples
  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.add_hide_scrollbar_property(css_code)
  updated css with .hide-scrollbar having display: none
  ```
  """
  def add_hide_scrollbar_property(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            import tinycss2
            from css_tools.modifier import add_property_to_selector

            try:
                # Ensure css_code is a string
                if isinstance(css_code, bytes):
                    css_code = css_code.decode('utf-8')

                # Try the modification
                modified_css = add_property_to_selector(
                    css_code,
                    ".hide-scrollbar",
                    "display",
                    "none"
                )
                result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => modified_css} ->
            {:ok, __ENV__.function, modified_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Adds vendor prefixes to specified CSS properties throughout the stylesheet.

  ## Parameters

    * `css_code` - The CSS code as a string
    * `property_name` - The CSS property to add prefixes to
    * `prefixes` - List of prefixes to add (e.g., ["-webkit-", "-moz-"])

  ## Examples

  ```elixir
  iex> prefixes = ["-webkit-", "-moz-", "-ms-"]
  iex> IgniterCss.Parsers.CSS.Parser.add_vendor_prefixes(css_code, "user-select", prefixes)
  "updated css with vendor prefixes"
  ```
  """
  def add_vendor_prefixes(file_path_or_content, property_name, prefixes, type \\ :content)
      when is_list(prefixes) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.modifier import add_prefix_to_property

            # Convert all prefixes from bytes to strings if needed
            string_prefixes = []
            for prefix in prefixes:
                if isinstance(prefix, bytes):
                    string_prefixes.append(prefix.decode('utf-8'))
                else:
                    string_prefixes.append(prefix)

            try:
              modified_css = add_prefix_to_property(
                  css_code,
                  property_name,
                  string_prefixes
              )

              result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{
              "css_code" => file_content,
              "property_name" => property_name,
              "prefixes" => prefixes
            }
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => modified_css} ->
            {:ok, __ENV__.function, modified_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Analyzes a CSS stylesheet and returns various statistics.

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.analyze_css(css_code)
  %{
    "selectors_count" => 15,
    "unique_selectors" => 12,
    "properties_count" => 45,
    "unique_properties" => 20,
    ...
  }
  ```
  """
  def analyze_css(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.parser import analyze_stylesheet
            try:
              analyze_css = analyze_stylesheet(css_code)

              result = {"status": "ok", "result": analyze_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => analyzed_css} ->
            {:ok, __ENV__.function, analyzed_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Extracts all color values from a CSS stylesheet.

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.extract_colors(css_code)
  %{
    ".header" => ["color: #333", "background-color: white"],
    ".footer" => ["color: rgba(0, 0, 0, 0.8)"]
  }
  ```
  """
  def extract_colors(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.extractor import extract_colors

            try:
              analyze_css = extract_colors(css_code)

              result = {"status": "ok", "result": analyze_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => analyzed_css} ->
            {:ok, __ENV__.function, analyzed_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Minifies a CSS stylesheet by removing comments, whitespace, and unnecessary characters.
  We recommend not using this.

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.minify(css_code)
  ".header{color:#333;background:#fff;}.footer{color:#000;}"
  ```
  """
  def minify(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.minifier import minify_css

            try:
              modified_css = minify_css(css_code)
              result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => modified_css} ->
            {:ok, __ENV__.function, modified_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Beautifies a CSS stylesheet by adding proper indentation and formatting.
  We recommend using `IgniterCss.Parsers.CSS.Formatter` module instead.

  ## Examples

      iex> IgniterCss.Parsers.CSS.Parser.beautify(css_code)
      ".header {
          color: #333;
          background: #fff;
      }

      .footer {
          color: #000;
      }"
  """
  def beautify(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.minifier import beautify_css

            try:
              modified_css = beautify_css(css_code)
              result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => modified_css} ->
            {:ok, __ENV__.function, modified_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Modifies a property value for a specific selector.

  ## Parameters

    * `css_code` - The CSS code as a string
    * `selector` - The CSS selector to modify
    * `property_name` - The property name to modify
    * `new_value` - The new property value
    * `important` - Whether to mark the property as !important (default: false)

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.modify_property(css_code, ".header", "color", "blue")
  "updated css with .header color: blue"
  ```
  """
  def modify_property(
        file_path_or_content,
        selector,
        property_name,
        new_value,
        important,
        type \\ :content
      )
      when is_boolean(important) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.modifier import modify_property_value

            try:
              modified_css = modify_property_value(
                  css_code,
                  selector,
                  property_name,
                  new_value,
                  important
              )

              result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{
              "css_code" => file_content,
              "selector" => selector,
              "property_name" => property_name,
              "new_value" => new_value,
              "important" => important
            }
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => modified_css} ->
            {:ok, __ENV__.function, modified_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Merges multiple CSS stylesheets into one, removing duplicates.

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.merge_stylesheets([css_code1, css_code2])
  "merged css"
  ```
  """
  def merge_stylesheets(css_list) when is_list(css_list) do
    {result, _globals} =
      Pythonx.eval(
        """
        from css_tools.modifier import merge_stylesheets

        try:
          modified_css = merge_stylesheets(css_list)
          result = {"status": "ok", "result": modified_css}

        except Exception as e:
            # Return any errors in a structured format
            result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

        result
        """,
        %{"css_list" => css_list}
      )

    parsed_result = Pythonx.decode(result)

    case parsed_result do
      %{"status" => "ok", "result" => modified_css} ->
        {:ok, __ENV__.function, modified_css}

      %{"status" => "error", "message" => message} ->
        {:error, __ENV__.function, message}
    end
  end

  @doc """
  Removes a CSS selector and all its properties.
  **Note**: If a block is empty after removal, it will be removed as well.

  ## Examples
  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.remove_selector(css_code, ".unused-class")
  "css without .unused-class"
  ```
  """
  def remove_selector(file_path_or_content, selector, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.modifier import remove_selector

            try:
              modified_css = remove_selector(css_code, selector)
              result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content, "selector" => selector}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => modified_css} ->
            {:ok, __ENV__.function, modified_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Extracts all media queries and their contents.

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.extract_media_queries(css_code)
  %{
    "(max-width: 768px)" => [
      %{
        "selector" => ".header",
        "properties" => %{"font-size" => "14px"}
      }
    ]
  }
  ```
  """
  def extract_media_queries(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.extractor import extract_media_queries

            try:
                modified_css = extract_media_queries(css_code)
                result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => analyzed_css} ->
            {:ok, __ENV__.function, analyzed_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Extracts all CSS animations and keyframes.

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.extract_animations(css_code)
  %{
    "fade-in" => %{
      "keyframes" => %{
        "0%" => %{"opacity" => "0"},
        "100%" => %{"opacity" => "1"}
      },
      "used_by" => [".header", ".modal"]
    }
  }
  ```
  """
  def extract_animations(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.extractor import extract_animations

            try:
              modified_css = extract_animations(css_code)
              result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => analyzed_css} ->
            {:ok, __ENV__.function, analyzed_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Sorts CSS properties alphabetically within each rule.

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.sort_properties(css_code)
  ".header {
      background: #fff;
      color: #333;
      font-size: 16px;
  }"
  ```
  """
  def sort_properties(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.minifier import sort_properties

            try:
              modified_css = sort_properties(css_code)
              result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => modified_css} ->
            {:ok, __ENV__.function, modified_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Removes duplicate selectors and properties from CSS.

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.remove_duplicates(css_code)
  "css without duplicates"
  ```
  """
  def remove_duplicates(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.minifier import remove_duplicates

            try:
              modified_css = remove_duplicates(css_code)
              result = {"status": "ok", "result": modified_css}

            except Exception as e:
                # Return any errors in a structured format
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => modified_css} ->
            {:ok, __ENV__.function, modified_css}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Checks if the CSS code is valid by attempting to parse it.
  Returns :ok if valid, or {:error, reason} if invalid.
  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.validate_css(css_code)
  :ok

  iex> IgniterCss.Parsers.CSS.Parser.validate_css("invalid { css")
  {:error, "Parse error at line 1, column 10: Missing closing brace"}
  ```
  """
  def validate_css(file_path_or_content, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            import tinycss2
            from css_tools.extractor import validate_css

            try:
                if isinstance(css_code, bytes):
                    css_code = css_code.decode('utf-8')
                # Use the validate_css function from extractor
                validate_css(css_code)
                result = {"valid": True, "message": "CSS is valid"}
            except Exception as e:
                result = {"valid": False, "message": str(e)}

            result
            """,
            %{"css_code" => file_content}
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"valid" => true} ->
            {:ok, __ENV__.function, true}

          %{"valid" => false, "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end

  @doc """
  Replaces an entire CSS rule for a specific selector with new declarations.

  ## Parameters

    * `css_code` - The CSS code as a string
    * `selector` - The CSS selector to replace
    * `new_declarations` - The new CSS declarations as a string (without curly braces)

  ## Examples

      iex> IgniterCss.Parsers.CSS.Parser.replace_selector_rule(css_code, ".header", "color: blue; font-size: 20px; padding: 10px;")
      "css with .header rule replaced"
  """
  def replace_selector_rule(file_path_or_content, selector, new_declarations, type \\ :content) do
    # First validate the CSS using the existing validate_css function
    case validate_css(file_path_or_content, type) do
      {:ok, _, _} ->
        # CSS is valid, proceed with replacement
        call_nif_fn(
          file_path_or_content,
          __ENV__.function,
          fn file_content ->
            {result, _globals} =
              Pythonx.eval(
                """
                from css_tools.modifier import replace_selector_rule
                try:
                    # Call the dedicated function
                    modified_css = replace_selector_rule(css_code, selector, new_declarations)
                    result = {"status": "ok", "result": modified_css}
                except Exception as e:
                    # Return any errors in a structured format
                    result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}
                result
                """,
                %{
                  "css_code" => file_content,
                  "selector" => selector,
                  "new_declarations" => new_declarations
                }
              )

            parsed_result = Pythonx.decode(result)

            case parsed_result do
              %{"status" => "ok", "result" => modified_css} ->
                {:ok, __ENV__.function, modified_css}

              %{"status" => "error", "message" => message} ->
                {:error, __ENV__.function, message}
            end
          end,
          type
        )

      # If validation fails, return the error
      {:error, _, error_message} ->
        {:error, :replace_selector_rule, error_message}
    end
  end

  @doc """
  Adds an @import rule to the CSS if it doesn't already exist.

  ## Parameters
    * `file_path_or_content` - The CSS code as a string or file path
    * `import_url` - The URL or path to import (without quotes)
    * `media_query` - Optional media query to apply to the import (e.g., "screen and (max-width: 768px)")
                      or boolean false to indicate no media query
    * `type` - `:content` or `:path` to specify if the first parameter is file content or a path

  ## Examples
      iex> IgniterCss.Parsers.CSS.Parser.add_import(css_code, "styles.css", false)
      {:ok, :add_import, "css with @import 'styles.css'; added"}

      iex> IgniterCss.Parsers.CSS.Parser.add_import(css_code, "mobile.css", "screen and (max-width: 768px)")
      {:ok, :add_import, "css with @import 'mobile.css' screen and (max-width: 768px); added"}
  """
  def add_import(file_path_or_content, import_url, media_query, type \\ :content)
      when is_boolean(media_query) or is_binary(media_query) or is_nil(media_query) do
    case validate_css(file_path_or_content, type) do
      {:ok, _, _} ->
        call_nif_fn(
          file_path_or_content,
          __ENV__.function,
          fn file_content ->
            {result, _globals} =
              Pythonx.eval(
                """
                import tinycss2
                from css_tools.parser import parse_stylesheet

                # Ensure we're working with strings
                if isinstance(css_code, bytes):
                    css_code = css_code.decode('utf-8')
                if isinstance(import_url, bytes):
                    import_url = import_url.decode('utf-8')

                # Handle the media query - only use it if it's a string and not a boolean
                media_query_str = ""
                if media_query is not None and not isinstance(media_query, bool):
                    if isinstance(media_query, bytes):
                        media_query = media_query.decode('utf-8')
                    media_query_str = f" {media_query}"

                # Format the import rule
                if import_url.startswith(("http://", "https://", "/")):
                    # URLs need to be quoted
                    new_import = f"@import url('{import_url}'){media_query_str};"
                else:
                    # Relative paths can be with or without quotes
                    new_import = f"@import '{import_url}'{media_query_str};"

                rules = parse_stylesheet(css_code)

                # Check if the import already exists
                exists = False
                for rule in rules:
                    if rule.type == "at-rule" and rule.at_keyword.lower() == "import":
                        if import_url in tinycss2.serialize(rule.prelude):
                            exists = True
                            break

                if exists:
                    # Don't add duplicate import
                    modified_css = css_code
                else:
                    # Add new import at the beginning - imports must come before other rules
                    has_imports = any(rule.type == "at-rule" and rule.at_keyword.lower() == "import" for rule in rules)

                    if has_imports:
                        # Add after the last import
                        modified_parts = []
                        last_import_index = -1

                        for i, rule in enumerate(rules):
                            if rule.type == "at-rule" and rule.at_keyword.lower() == "import":
                                last_import_index = i

                        # Add all rules up to the last import
                        for i, rule in enumerate(rules):
                            part = tinycss2.serialize([rule])
                            # Ensure this is a string
                            if isinstance(part, bytes):
                                part = part.decode('utf-8')
                            modified_parts.append(part)

                            if i == last_import_index:
                                # Add the new import after the last existing import
                                modified_parts.append(f"\\n{new_import}\\n")

                        modified_css = "".join(modified_parts)
                    else:
                        # No existing imports, add at the beginning
                        # Make sure to convert any bytes to strings
                        if isinstance(css_code, bytes):
                            css_code = css_code.decode('utf-8')
                        modified_css = f"{new_import}\\n{css_code}"

                # Final check to ensure we return a string, not bytes
                if isinstance(modified_css, bytes):
                    modified_css = modified_css.decode('utf-8')

                result = {"status": "ok", "result": modified_css.strip()}
                result
                """,
                %{
                  "css_code" => file_content,
                  "import_url" => import_url,
                  "media_query" => media_query
                }
              )

            parsed_result = Pythonx.decode(result)

            case parsed_result do
              %{"status" => "ok", "result" => modified_css} ->
                {:ok, __ENV__.function, modified_css}

              %{"status" => "error", "message" => message} ->
                {:error, __ENV__.function, message}
            end
          end,
          type
        )

      {:error, _, error_message} ->
        {:error, :add_import, error_message}
    end
  end

  @doc """
  Removes a specific @import rule from the CSS.

  ## Parameters

    * `css_code` - The CSS code as a string
    * `import_url` - The URL or path to remove (matches partial URL)

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.remove_import(css_code, "styles.css")
  "css with @import url('styles.css') removed"
  ```
  """
  def remove_import(file_path_or_content, import_url, type \\ :content) do
    case validate_css(file_path_or_content, type) do
      {:ok, _, _} ->
        call_nif_fn(
          file_path_or_content,
          __ENV__.function,
          fn file_content ->
            {result, _globals} =
              Pythonx.eval(
                """
                import tinycss2
                from css_tools.parser import parse_stylesheet

                if isinstance(import_url, bytes):
                    import_url = import_url.decode('utf-8')

                try:
                  rules = parse_stylesheet(css_code)
                  modified_css = ""

                  for rule in rules:
                      if rule.type == "at-rule" and rule.at_keyword.lower() == "import":
                          # Check if this import contains the URL we want to remove
                          serialized = tinycss2.serialize(rule.prelude)
                          if import_url not in serialized:
                              # Keep imports that don't match
                              modified_css += tinycss2.serialize([rule])
                      else:
                          # Keep all other rules
                          modified_css += tinycss2.serialize([rule])

                  modified_css = modified_css.strip()
                  result = {"status": "ok", "result": modified_css}

                except Exception as e:
                    # Return any errors in a structured format
                    result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

                result
                """,
                %{
                  "css_code" => file_content,
                  "import_url" => import_url
                }
              )

            parsed_result = Pythonx.decode(result)

            case parsed_result do
              %{"status" => "ok", "result" => modified_css} ->
                {:ok, __ENV__.function, modified_css}

              %{"status" => "error", "message" => message} ->
                {:error, __ENV__.function, message}
            end
          end,
          type
        )

      {:error, _, error_message} ->
        {:error, :remove_import, error_message}
    end
  end

  @doc """
  Checks if a specific CSS selector exists in the stylesheet.

  ## Parameters

    * `css_code` - The CSS code as a string
    * `selector` - The CSS selector to check for

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.selector_exists?(css_code, ".header")
  true

  iex> IgniterCss.Parsers.CSS.Parser.selector_exists?(css_code, "#nonexistent")
  false
  ```
  """
  def selector_exists?(file_path_or_content, selector, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            from css_tools.parser import parse_stylesheet, get_selector_text

            if isinstance(selector, bytes):
                selector = selector.decode('utf-8')

            rules = parse_stylesheet(css_code)
            exists = False

            for rule in rules:
                if rule.type == "qualified-rule":
                    rule_selector = get_selector_text(rule)
                    if rule_selector == selector:
                        exists = True
                        break

            result = exists
            result
            """,
            %{
              "css_code" => file_content,
              "selector" => selector
            }
          )

        parsed_result = Pythonx.decode(result)

        if parsed_result,
          do: {:ok, __ENV__.function, true},
          else: {:error, __ENV__.function, false}
      end,
      type
    )
  rescue
    _ -> {:error, __ENV__.function, false}
  end

  @doc """
  Gets the CSS properties for a specific selector if it exists, or returns nil.

  ## Parameters

    * `css_code` - The CSS code as a string
    * `selector` - The CSS selector to check for

  ## Examples

  ```elixir
  iex> IgniterCss.Parsers.CSS.Parser.get_selector_properties(css_code, ".header")
  %{"color" => "blue", "font-size" => "16px"}

  iex> IgniterCss.Parsers.CSS.Parser.get_selector_properties(css_code, "#nonexistent")
  nil
  ```
  """

  def get_selector_properties(file_path_or_content, selector, type \\ :content) do
    call_nif_fn(
      file_path_or_content,
      __ENV__.function,
      fn file_content ->
        {result, _globals} =
          Pythonx.eval(
            """
            import tinycss2
            from css_tools.parser import parse_stylesheet, get_selector_text, get_rule_declarations

            try:
                if isinstance(selector, bytes):
                    selector = selector.decode('utf-8')

                rules = parse_stylesheet(css_code)
                properties = None

                for rule in rules:
                    if rule.type == "qualified-rule":
                        rule_selector = get_selector_text(rule)
                        if rule_selector == selector:
                            declarations = get_rule_declarations(rule)
                            properties = {}
                            for decl in declarations:
                                if decl.type == "declaration":
                                    value = tinycss2.serialize(decl.value).strip()
                                    properties[decl.name] = value
                            break

                result = {"status": "ok", "result": properties}
            except Exception as e:
                result = {"status": "error", "message": f"Failed to parse CSS: {str(e)}"}

            result
            """,
            %{
              "css_code" => file_content,
              "selector" => selector
            }
          )

        parsed_result = Pythonx.decode(result)

        case parsed_result do
          %{"status" => "ok", "result" => properties} ->
            {:ok, __ENV__.function, properties}

          %{"status" => "error", "message" => message} ->
            {:error, __ENV__.function, message}
        end
      end,
      type
    )
  end
end
