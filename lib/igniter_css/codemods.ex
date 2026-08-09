# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.Codemods do
  @moduledoc """
  Igniter-facing wrappers: they take and return an `Igniter` struct, so callers
  get Igniter's normal diff preview and confirmation flow for free.

  Available only when `igniter` is a dependency. Without it, every function here
  returns `{:error, :igniter_not_available}`; use `IgniterCss` directly instead.

  ## Comment ownership on delete

  When a codemod removes a declaration or a rule, these comments go with it:

  * a comment **trailing on the same line**;
  * a comment on its **own line directly above**, with no blank line between.

  These are kept:

  * a comment separated from the target by a **blank line**;
  * a comment that reads as a **section header** — one spanning several lines, or
    containing a rule of three or more repeated `= - * # ~ _` characters, e.g.
    `/* ===== Layout ===== */`.

  ## Example

      def install(igniter, _opts) do
        igniter
        |> IgniterCss.Codemods.ensure_at_rule("assets/css/app.css", ~s|@plugin "daisyui";|)
        |> IgniterCss.Codemods.ensure_rule("assets/css/app.css", ".hide-scrollbar")
        |> IgniterCss.Codemods.set_declaration(
          "assets/css/app.css", ".hide-scrollbar", "scrollbar-width", "none"
        )
      end

  Each step is idempotent, so re-running the installer produces no diff.
  """

  @igniter_available Code.ensure_loaded?(Igniter)

  if @igniter_available do
    @doc "See `IgniterCss.ensure_at_rule/3`."
    def ensure_at_rule(igniter, path, line, opts \\ []) do
      update(igniter, path, "ensure_at_rule #{inspect(line)}", fn source ->
        IgniterCss.ensure_at_rule(source, line, opts)
      end)
    end

    @doc "See `IgniterCss.remove_at_rule/4`."
    def remove_at_rule(igniter, path, name, matching \\ nil, opts \\ []) do
      update(igniter, path, "remove_at_rule #{inspect(name)}", fn source ->
        IgniterCss.remove_at_rule(source, name, matching, opts)
      end)
    end

    @doc "See `IgniterCss.add_import/4`."
    def add_import(igniter, path, url, media \\ nil, opts \\ []) do
      update(igniter, path, "add_import #{inspect(url)}", fn source ->
        IgniterCss.add_import(source, url, media, opts)
      end)
    end

    @doc "See `IgniterCss.remove_import/3`."
    def remove_import(igniter, path, url, opts \\ []) do
      update(igniter, path, "remove_import #{inspect(url)}", fn source ->
        IgniterCss.remove_import(source, url, opts)
      end)
    end

    @doc "See `IgniterCss.ensure_rule/4`."
    def ensure_rule(igniter, path, selector, declarations \\ "", opts \\ []) do
      update(igniter, path, "ensure_rule #{inspect(selector)}", fn source ->
        IgniterCss.ensure_rule(source, selector, declarations, opts)
      end)
    end

    @doc "See `IgniterCss.remove_rule/3`."
    def remove_rule(igniter, path, selector, opts \\ []) do
      update(igniter, path, "remove_rule #{inspect(selector)}", fn source ->
        IgniterCss.remove_rule(source, selector, opts)
      end)
    end

    @doc "See `IgniterCss.set_declaration/5`."
    def set_declaration(igniter, path, selector, property, value, opts \\ []) do
      label = "set_declaration #{inspect(selector)} #{inspect(property)}"

      update(igniter, path, label, fn source ->
        IgniterCss.set_declaration(source, selector, property, value, opts)
      end)
    end

    @doc "See `IgniterCss.remove_declaration/4`."
    def remove_declaration(igniter, path, selector, property, opts \\ []) do
      label = "remove_declaration #{inspect(selector)} #{inspect(property)}"

      update(igniter, path, label, fn source ->
        IgniterCss.remove_declaration(source, selector, property, opts)
      end)
    end

    @doc "See `IgniterCss.append_raw_to_rule/4`."
    def append_raw_to_rule(igniter, path, selector, raw, opts \\ []) do
      update(igniter, path, "append_raw_to_rule #{inspect(selector)}", fn source ->
        IgniterCss.append_raw_to_rule(source, selector, raw, opts)
      end)
    end

    @doc "See `IgniterCss.add_vendor_prefixes/4`."
    def add_vendor_prefixes(igniter, path, property, prefixes, opts \\ []) do
      update(igniter, path, "add_vendor_prefixes #{inspect(property)}", fn source ->
        IgniterCss.add_vendor_prefixes(source, property, prefixes, opts)
      end)
    end

    # A codemod that cannot be applied raises rather than silently skipping.
    # An installer that quietly leaves a file unpatched is worse than one that
    # stops and says which file and which operation failed.
    defp update(igniter, path, label, fun) do
      Igniter.update_file(igniter, path, fn source ->
        content = Rewrite.Source.get(source, :content)

        case fun.(content) do
          {:ok, %IgniterCss.Outcome{changed: false}} ->
            source

          {:ok, %IgniterCss.Outcome{source: patched}} ->
            Rewrite.Source.update(source, :content, patched)

          {:error, reason} ->
            raise "igniter_css: #{label} failed on #{path}: #{inspect(reason)}"
        end
      end)
    end
  else
    for {name, arity} <- [
          ensure_at_rule: 4,
          remove_at_rule: 5,
          add_import: 5,
          remove_import: 4,
          ensure_rule: 5,
          remove_rule: 4,
          set_declaration: 6,
          remove_declaration: 5,
          append_raw_to_rule: 5,
          add_vendor_prefixes: 5
        ] do
      @doc false
      def unquote(name)(unquote_splicing(Macro.generate_arguments(arity, __MODULE__))) do
        {:error, :igniter_not_available}
      end
    end
  end
end
