# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.ParseOpts do
  @moduledoc """
  Parser options handed to the native layer.

  * `:allow_wrong_line_comments` — treat `//` as a comment. On by default: the
    habit leaks in from css-in-js tooling often enough to be worth tolerating,
    and it cannot lose data either way (with the flag off those bytes become
    error-tolerant nodes that still carry their text).
  * `:css_modules` — enable CSS Modules syntax (`:global`, `composes`).
  """

  @type t :: %__MODULE__{
          allow_wrong_line_comments: boolean(),
          css_modules: boolean()
        }

  defstruct allow_wrong_line_comments: true, css_modules: false

  @doc """
  Build options from a keyword list, falling back to the defaults.

      iex> IgniterCss.ParseOpts.new(css_modules: true).css_modules
      true
  """
  @spec new(keyword() | t()) :: t()
  def new(%__MODULE__{} = opts), do: opts

  def new(opts) when is_list(opts) do
    %__MODULE__{
      allow_wrong_line_comments:
        Keyword.get(opts, :allow_wrong_line_comments, true) |> normalize_bool(true),
      css_modules: Keyword.get(opts, :css_modules, false) |> normalize_bool(false)
    }
  end

  defp normalize_bool(value, _default) when is_boolean(value), do: value
  defp normalize_bool(_value, default), do: default
end

defmodule IgniterCss.Outcome do
  @moduledoc """
  The result of a codemod.

  `changed?` is authoritative: it is `false` exactly when `source` is
  byte-identical to the input, which is what makes every op safe to re-run in an
  Igniter installer.
  """

  @type t :: %__MODULE__{
          source: String.t(),
          changed: boolean(),
          diagnostics: [String.t()]
        }

  defstruct source: "", changed: false, diagnostics: []
end

defmodule IgniterCss.Analysis do
  @moduledoc """
  Statistics about a stylesheet. Read-only; produced by `IgniterCss.analyze/2`.
  """

  @type t :: %__MODULE__{
          rules_count: non_neg_integer(),
          top_level_rules_count: non_neg_integer(),
          selectors_count: non_neg_integer(),
          unique_selectors: non_neg_integer(),
          declarations_count: non_neg_integer(),
          unique_properties: non_neg_integer(),
          at_rules_count: non_neg_integer(),
          media_queries_count: non_neg_integer(),
          keyframes_count: non_neg_integer(),
          imports_count: non_neg_integer(),
          comments_count: non_neg_integer(),
          colors_count: non_neg_integer(),
          important_count: non_neg_integer(),
          custom_properties_count: non_neg_integer(),
          property_frequency: [{String.t(), non_neg_integer()}],
          selectors: [String.t()],
          at_rule_names: [String.t()]
        }

  defstruct rules_count: 0,
            top_level_rules_count: 0,
            selectors_count: 0,
            unique_selectors: 0,
            declarations_count: 0,
            unique_properties: 0,
            at_rules_count: 0,
            media_queries_count: 0,
            keyframes_count: 0,
            imports_count: 0,
            comments_count: 0,
            colors_count: 0,
            important_count: 0,
            custom_properties_count: 0,
            property_frequency: [],
            selectors: [],
            at_rule_names: []
end

defmodule IgniterCss.Validation do
  @moduledoc """
  Whether a stylesheet is understood well enough to patch.

  `round_trips` is the half that matters for safety: it is what every codemod
  checks before touching a file. A stylesheet that round-trips but has
  diagnostics is still patchable; one that does not round-trip is not, and
  `IgniterCss` will refuse rather than risk a wrong edit.
  """

  @type t :: %__MODULE__{
          valid: boolean(),
          diagnostics: non_neg_integer(),
          round_trips: boolean(),
          message: String.t()
        }

  defstruct valid: false, diagnostics: 0, round_trips: false, message: ""
end

defmodule IgniterCss.AtRule do
  @moduledoc """
  One at-rule, read back whole.

  `IgniterCss.analyze/2` reports only that an at-rule of some name exists. This
  reports which one and how it was configured — the difference between knowing a
  `@plugin` is present and knowing it was loaded as
  `@plugin "daisyui" { prefix: "d-"; }`, which is what an installer needs before
  it can generate code that agrees with the user's setup.

  `declarations` is empty for an at-rule with no block, or one whose block holds
  rules rather than declarations.
  """

  @type t :: %__MODULE__{
          name: String.t(),
          prelude: String.t(),
          target: String.t() | nil,
          has_block: boolean(),
          declarations: [{String.t(), String.t()}],
          text: String.t(),
          body: String.t() | nil
        }

  defstruct name: "",
            prelude: "",
            target: nil,
            has_block: false,
            declarations: [],
            text: "",
            body: nil
end

defmodule IgniterCss.Animation do
  @moduledoc """
  A `@keyframes` animation and the selectors that use it.
  """

  @type t :: %__MODULE__{
          name: String.t(),
          keyframes: [{String.t(), [{String.t(), String.t()}]}],
          used_by: [String.t()]
        }

  defstruct name: "", keyframes: [], used_by: []
end
