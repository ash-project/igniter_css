# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss do
  @moduledoc """
  Semantic patches for CSS files that a user owns, powered by a Rust parser
  (Biome's lossless CSS CST) integrated via NIFs.

  This is a **codemod** tool, not a formatter, minifier or bundler. Every
  operation returns the user's original file with only the intended bytes
  changed.

  ## Guarantees

  Four properties hold for every function in this module:

  1. **Comments are never lost.** Not mostly preserved — never lost. The
     implementation edits byte ranges rather than reprinting a tree, so text
     outside an edit cannot change.
  2. **Diffs are minimal.** `git diff` after a codemod shows only the lines the
     codemod meant to change. There is no whole-file reformatting, ever.
  3. **Everything is idempotent.** Applying an operation twice produces the same
     result as applying it once, and the second run reports `changed: false`.
     Igniter installers get re-run; this is not optional.
  4. **Input is never destroyed.** If a file cannot be understood well enough to
     patch safely — the parse does not reproduce it byte for byte, or its braces
     are unbalanced — you get `{:error, reason}` and the file is untouched.

  ## Shape

  Mutating functions return `{:ok, %IgniterCss.Outcome{}}` or `{:error, reason}`:

      {:ok, %IgniterCss.Outcome{source: "...", changed: true, diagnostics: []}}

  Query functions return `{:ok, value}` or `{:error, reason}`.

  Every function takes an optional trailing keyword list, forwarded to
  `IgniterCss.ParseOpts`.

  ## Selector matching

  Matching is deliberately strict, because guessing is how a codemod produces a
  surprising diff:

  * only **top-level** rules are matched — `.b` inside `@media print` is not
    found by `set_declaration/5`;
  * selectors are compared on a normalised form (`.a>.b` matches `.a > .b`),
    never on raw equality and never on a substring or fuzzy basis;
  * a selector list is matched as a whole — `.a` does not match `.a, .b`;
  * if **more than one** top-level rule matches, you get an error rather than an
    arbitrary choice.

  ## Examples

      iex> {:ok, out} = IgniterCss.ensure_at_rule("", ~s|@plugin "daisyui";|)
      iex> out.source
      ~s|@plugin "daisyui";\\n|

      iex> css = ".btn {\\n  color: red; /* brand */\\n}\\n"
      iex> {:ok, out} = IgniterCss.set_declaration(css, ".btn", "color", "var(--brand)")
      iex> out.source
      ".btn {\\n  color: var(--brand); /* brand */\\n}\\n"

      iex> css = ".btn {\\n  color: red;\\n}\\n"
      iex> {:ok, out} = IgniterCss.set_declaration(css, ".btn", "color", "red")
      iex> out.changed
      false

  ## What is not here

  `minify/2`, `beautify/2` and `merge_stylesheets/2` live in
  `IgniterCss.Transform`. They rewrite the whole file by design, so they are
  kept away from the codemods and must not be used to patch a user's stylesheet.
  """

  alias IgniterCss.{Analysis, Animation, Native, Outcome, ParseOpts, Validation}

  @type opts :: keyword()
  @type result :: {:ok, Outcome.t()} | {:error, String.t()}

  # ---------------------------------------------------------------------------
  # At-rules
  # ---------------------------------------------------------------------------

  @doc """
  Insert a top-level at-rule line unless an equivalent one is already present.

  The insertion anchor is the last existing at-rule of the same name; failing
  that, the end of the file's at-rule prologue; failing that, the top of the
  file but below any header comment. `@import`, `@charset`, `@use` and
  `@namespace` are never placed after a style rule.

  Two at-rules of the same name naming the same target count as the same rule,
  so `@import "tailwindcss";` is not added again to a file that already says
  `@import "tailwindcss" source(none);`.

      iex> css = ~s|@import "tailwindcss";\\n@source "../js";\\n|
      iex> {:ok, out} = IgniterCss.ensure_at_rule(css, ~s|@plugin "daisyui";|)
      iex> out.source
      ~s|@import "tailwindcss";\\n@source "../js";\\n@plugin "daisyui";\\n|
  """
  @spec ensure_at_rule(String.t(), String.t(), opts()) :: result()
  def ensure_at_rule(source, line, opts \\ []) do
    Native.ensure_at_rule_nif(source, line, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Remove top-level at-rules of `name`.

  `matching` filters by target (or, failing that, by the whole prelude); `nil`
  removes every at-rule with that name. Comments the removed rule owns go with
  it — see `IgniterCss.Codemods` for the ownership rules.
  """
  @spec remove_at_rule(String.t(), String.t(), String.t() | nil, opts()) :: result()
  def remove_at_rule(source, name, matching \\ nil, opts \\ []) do
    Native.remove_at_rule_nif(source, name, matching, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Is an at-rule equivalent to `line` already present at the top level?

      iex> IgniterCss.has_at_rule?(~s|@plugin "a";\\n|, ~s|@plugin "a";|)
      {:ok, true}
  """
  @spec has_at_rule?(String.t(), String.t(), opts()) :: {:ok, boolean()} | {:error, String.t()}
  def has_at_rule?(source, line, opts \\ []) do
    Native.has_at_rule_nif(source, line, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Every top-level at-rule named `name`, as `IgniterCss.AtRule` structs.

  Pass `matching` to narrow to a single target — the string or `url()` before
  the block. Returns `[]` when nothing matches; absence is an answer, not an
  error.

  Unlike `has_at_rule?/3`, this hands back the at-rule's block, so a caller can
  read a decision out of it rather than only confirm the line exists:

      iex> css = ~s|@plugin "daisyui" { prefix: "d-"; }|
      iex> {:ok, [rule]} = IgniterCss.get_at_rules(css, "plugin", "daisyui")
      iex> rule.declarations
      [{"prefix", ~s|"d-"|}]

  The leading `@` in `name` is optional.
  """
  @spec get_at_rules(String.t(), String.t(), String.t() | nil, opts()) ::
          {:ok, [IgniterCss.AtRule.t()]} | {:error, String.t()}
  def get_at_rules(source, name, matching \\ nil, opts \\ []) do
    Native.get_at_rules_nif(source, name, matching, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Add an `@import`, building the line for you.

  Absolute URLs are wrapped in `url(...)`; relative paths are quoted.
  """
  @spec add_import(String.t(), String.t(), String.t() | nil, opts()) :: result()
  def add_import(source, url, media \\ nil, opts \\ []) do
    Native.add_import_nif(source, url, media, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Remove `@import` rules pointing at `url`, however they were written
  (`"x.css"`, `'x.css'` and `url("x.css")` all match).
  """
  @spec remove_import(String.t(), String.t(), opts()) :: result()
  def remove_import(source, url, opts \\ []) do
    Native.remove_import_nif(source, url, ParseOpts.new(opts)) |> unwrap()
  end

  # ---------------------------------------------------------------------------
  # Rules
  # ---------------------------------------------------------------------------

  @doc """
  Create `selector { }` at the end of the file when no top-level rule with that
  selector exists. Pass `declarations` to seed the body.

      iex> {:ok, out} = IgniterCss.ensure_rule("", ".hide-scrollbar", "display: none")
      iex> out.source
      ".hide-scrollbar {\\n  display: none;\\n}\\n"
  """
  @spec ensure_rule(String.t(), String.t(), String.t(), opts()) :: result()
  def ensure_rule(source, selector, declarations \\ "", opts \\ []) do
    Native.ensure_rule_nif(source, selector, declarations, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Remove every top-level rule with this selector, plus the comments it owns.
  """
  @spec remove_rule(String.t(), String.t(), opts()) :: result()
  def remove_rule(source, selector, opts \\ []) do
    Native.remove_rule_nif(source, selector, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Replace everything between a rule's braces.

  Errors when the selector matches no top-level rule, or more than one.
  """
  @spec replace_rule_body(String.t(), String.t(), String.t(), opts()) :: result()
  def replace_rule_body(source, selector, declarations, opts \\ []) do
    Native.replace_rule_body_nif(source, selector, declarations, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Append caller-provided raw text to the end of a rule body, re-indented to
  match the surrounding code. A no-op if the text is already in the body.
  """
  @spec append_raw_to_rule(String.t(), String.t(), String.t(), opts()) :: result()
  def append_raw_to_rule(source, selector, raw, opts \\ []) do
    Native.append_raw_to_rule_nif(source, selector, raw, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Does a top-level rule with this selector exist?

      iex> IgniterCss.has_rule?(".a  >  .b { color: red; }", ".a>.b")
      {:ok, true}
  """
  @spec has_rule?(String.t(), String.t(), opts()) :: {:ok, boolean()} | {:error, String.t()}
  def has_rule?(source, selector, opts \\ []) do
    Native.has_rule_nif(source, selector, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Every top-level selector, exactly as written.
  """
  @spec list_selectors(String.t(), opts()) :: {:ok, [String.t()]} | {:error, String.t()}
  def list_selectors(source, opts \\ []) do
    Native.list_selectors_nif(source, ParseOpts.new(opts)) |> unwrap()
  end

  # ---------------------------------------------------------------------------
  # Declarations
  # ---------------------------------------------------------------------------

  @doc """
  Set a property inside the rule matching `selector`.

  If the property is already there, **only its value bytes are replaced** — an
  inline comment on that line and any `!important` you did not ask to change
  both survive. If it is not, a new declaration is appended in the file's own
  indentation and newline style.

  ## Options

  * `:important` — `true` adds `!important`, `false` removes it, `nil` (the
    default) leaves whatever is there.
  * `:create_rule` — when `true`, a missing rule is created instead of being an
    error. Defaults to `false`.

  ## Examples

      iex> css = ".btn { color: red !important; }"
      iex> {:ok, out} = IgniterCss.set_declaration(css, ".btn", "color", "blue")
      iex> out.source
      ".btn { color: blue !important; }"

      iex> {:ok, out} = IgniterCss.set_declaration("", ".x", "display", "none", create_rule: true)
      iex> out.source
      ".x {\\n  display: none;\\n}\\n"
  """
  @spec set_declaration(String.t(), String.t(), String.t(), String.t(), opts()) :: result()
  def set_declaration(source, selector, property, value, opts \\ []) do
    important = Keyword.get(opts, :important)
    create_rule = Keyword.get(opts, :create_rule, false) == true

    Native.set_declaration_nif(
      source,
      selector,
      property,
      value,
      important,
      create_rule,
      ParseOpts.new(opts)
    )
    |> unwrap()
  end

  @doc """
  Remove every declaration of `property` from the rule matching `selector`,
  together with the comments those declarations own.

  Removing from a rule that does not exist is a no-op, not an error.
  """
  @spec remove_declaration(String.t(), String.t(), String.t(), opts()) :: result()
  def remove_declaration(source, selector, property, opts \\ []) do
    Native.remove_declaration_nif(source, selector, property, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  The value of `property` in the rule matching `selector`, as written, or `nil`.

      iex> IgniterCss.get_declaration(".a { color: red !important; }", ".a", "color")
      {:ok, "red !important"}
  """
  @spec get_declaration(String.t(), String.t(), String.t(), opts()) ::
          {:ok, String.t() | nil} | {:error, String.t()}
  def get_declaration(source, selector, property, opts \\ []) do
    Native.get_declaration_nif(source, selector, property, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Does the rule matching `selector` set `property`?
  """
  @spec has_declaration?(String.t(), String.t(), String.t(), opts()) ::
          {:ok, boolean()} | {:error, String.t()}
  def has_declaration?(source, selector, property, opts \\ []) do
    Native.has_declaration_nif(source, selector, property, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Every declaration in the rule matching `selector`, as `{property, value}`
  pairs in source order, or `nil` when the rule does not exist.

      iex> IgniterCss.get_rule_declarations(".a { color: red; margin: 0; }", ".a")
      {:ok, [{"color", "red"}, {"margin", "0"}]}
  """
  @spec get_rule_declarations(String.t(), String.t(), opts()) ::
          {:ok, [{String.t(), String.t()}] | nil} | {:error, String.t()}
  def get_rule_declarations(source, selector, opts \\ []) do
    Native.get_rule_declarations_nif(source, selector, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Add vendor-prefixed copies of `property` next to every occurrence of it in the
  file.

  Prefixed declarations go immediately **before** the standard one, which is the
  ordering browsers expect. Prefixes already present in the same block are
  skipped, so re-running is a no-op.

      iex> {:ok, out} = IgniterCss.add_vendor_prefixes(".a { user-select: none; }", "user-select", ["-webkit-"])
      iex> out.source
      ".a { -webkit-user-select: none; user-select: none; }"
  """
  @spec add_vendor_prefixes(String.t(), String.t(), [String.t()], opts()) :: result()
  def add_vendor_prefixes(source, property, prefixes, opts \\ []) when is_list(prefixes) do
    Native.add_vendor_prefixes_nif(source, property, prefixes, ParseOpts.new(opts)) |> unwrap()
  end

  # ---------------------------------------------------------------------------
  # Tidying
  # ---------------------------------------------------------------------------

  @doc """
  Sort declarations alphabetically within each block, by moving whole lines.

  Comments move with the declaration they belong to. A block that cannot be
  rearranged safely — declarations sharing a line, a nested rule, a section
  header between declarations — is left alone and reported in
  `outcome.diagnostics`.

  Note this is a semantic change when a block mixes shorthand and longhand
  (`margin` before `margin-left` behaves differently from the reverse).
  """
  @spec sort_properties(String.t(), opts()) :: result()
  def sort_properties(source, opts \\ []) do
    Native.sort_properties_nif(source, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Remove redundant declarations and rules.

  Only removals that cannot change rendering are made: a declaration goes only
  when a later one in the same block sets the same property and is at least as
  important, and a rule goes only when a later top-level rule has the same
  selector *and* a byte-identical body.

  ## Options

  * `:declarations` — defaults to `true`
  * `:rules` — defaults to `true`
  """
  @spec remove_duplicates(String.t(), opts()) :: result()
  def remove_duplicates(source, opts \\ []) do
    declarations = Keyword.get(opts, :declarations, true) != false
    rules = Keyword.get(opts, :rules, true) != false

    Native.remove_duplicates_nif(source, declarations, rules, ParseOpts.new(opts)) |> unwrap()
  end

  # ---------------------------------------------------------------------------
  # Analysis (read-only)
  # ---------------------------------------------------------------------------

  @doc """
  Statistics about a stylesheet.

      iex> {:ok, a} = IgniterCss.analyze(".a { color: red; }")
      iex> {a.rules_count, a.declarations_count}
      {1, 1}
  """
  @spec analyze(String.t(), opts()) :: {:ok, Analysis.t()} | {:error, String.t()}
  def analyze(source, opts \\ []) do
    Native.analyze_nif(source, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Is this stylesheet understood well enough to patch?

  Returns `{:ok, %IgniterCss.Validation{}}` when it is and
  `{:error, %IgniterCss.Validation{}}` when it is not, so the details are
  available either way.
  """
  @spec validate(String.t(), opts()) :: {:ok, Validation.t()} | {:error, Validation.t()}
  def validate(source, opts \\ []) do
    case Native.validate_nif(source, ParseOpts.new(opts)) do
      {:ok, _fun, validation} -> {:ok, validation}
      {:error, _fun, validation} -> {:error, validation}
    end
  end

  @doc """
  Colour-carrying declarations, grouped by the selector they belong to.

      iex> IgniterCss.extract_colors(".a { color: #333; margin: 0; }")
      {:ok, [{".a", ["color: #333"]}]}
  """
  @spec extract_colors(String.t(), opts()) ::
          {:ok, [{String.t(), [String.t()]}]} | {:error, String.t()}
  def extract_colors(source, opts \\ []) do
    Native.extract_colors_nif(source, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  Media queries in the file, each with the rules it contains.
  """
  @spec extract_media_queries(String.t(), opts()) ::
          {:ok, [{String.t(), [{String.t(), [{String.t(), String.t()}]}]}]}
          | {:error, String.t()}
  def extract_media_queries(source, opts \\ []) do
    Native.extract_media_queries_nif(source, ParseOpts.new(opts)) |> unwrap()
  end

  @doc """
  `@keyframes` animations, their steps, and the selectors that use them.
  """
  @spec extract_animations(String.t(), opts()) :: {:ok, [Animation.t()]} | {:error, String.t()}
  def extract_animations(source, opts \\ []) do
    Native.extract_animations_nif(source, ParseOpts.new(opts)) |> unwrap()
  end

  # ---------------------------------------------------------------------------

  defp unwrap({:ok, _fun, value}), do: {:ok, value}
  defp unwrap({:error, _fun, reason}), do: {:error, reason}
end
