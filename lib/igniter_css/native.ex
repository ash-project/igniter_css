# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.Native do
  @moduledoc false
  # Precompiled NIFs, so end users never need a Rust toolchain
  # (hard constraint #5). Set IGNITERCSS_BUILD=1 to force a local build.

  mix_config = Mix.Project.config()
  version = mix_config[:version]
  github_url = mix_config[:package][:links]["GitHub"]

  use RustlerPrecompiled,
    otp_app: :igniter_css,
    crate: "igniter_css",
    base_url: "#{github_url}/releases/download/v#{version}",
    version: version,
    targets: ~w(
        aarch64-apple-darwin
        aarch64-unknown-linux-gnu
        aarch64-unknown-linux-musl
        riscv64gc-unknown-linux-gnu
        x86_64-apple-darwin
        x86_64-pc-windows-gnu
        x86_64-pc-windows-msvc
        x86_64-unknown-freebsd
        x86_64-unknown-linux-gnu
        x86_64-unknown-linux-musl
      ),
    force_build:
      System.get_env("IGNITERCSS_BUILD") in ["1", "true"] ||
        System.get_env("ASH_CI_BUILD") in ["1", "true"]

  # -- at-rules ---------------------------------------------------------------

  def ensure_at_rule_nif(_source, _line, _opts), do: error()
  def remove_at_rule_nif(_source, _name, _matching, _opts), do: error()
  def has_at_rule_nif(_source, _line, _opts), do: error()
  def add_import_nif(_source, _url, _media, _opts), do: error()
  def remove_import_nif(_source, _url, _opts), do: error()

  # -- rules ------------------------------------------------------------------

  def ensure_rule_nif(_source, _selector, _declarations, _opts), do: error()
  def remove_rule_nif(_source, _selector, _opts), do: error()
  def replace_rule_body_nif(_source, _selector, _declarations, _opts), do: error()
  def append_raw_to_rule_nif(_source, _selector, _raw, _opts), do: error()
  def has_rule_nif(_source, _selector, _opts), do: error()
  def list_selectors_nif(_source, _opts), do: error()

  # -- declarations -----------------------------------------------------------

  def set_declaration_nif(
        _source,
        _selector,
        _property,
        _value,
        _important,
        _create_rule,
        _opts
      ),
      do: error()

  def remove_declaration_nif(_source, _selector, _property, _opts), do: error()
  def get_declaration_nif(_source, _selector, _property, _opts), do: error()
  def has_declaration_nif(_source, _selector, _property, _opts), do: error()
  def get_rule_declarations_nif(_source, _selector, _opts), do: error()
  def add_vendor_prefixes_nif(_source, _property, _prefixes, _opts), do: error()

  # -- tidy -------------------------------------------------------------------

  def sort_properties_nif(_source, _opts), do: error()
  def remove_duplicates_nif(_source, _declarations, _rules, _opts), do: error()

  # -- analysis ---------------------------------------------------------------

  def analyze_nif(_source, _opts), do: error()
  def validate_nif(_source, _opts), do: error()
  def extract_colors_nif(_source, _opts), do: error()
  def extract_media_queries_nif(_source, _opts), do: error()
  def extract_animations_nif(_source, _opts), do: error()

  # -- whole-file transforms --------------------------------------------------

  def minify_nif(_source, _opts), do: error()
  def beautify_nif(_source, _opts), do: error()
  def merge_stylesheets_nif(_sources, _opts), do: error()

  defp error, do: :erlang.nif_error(:nif_not_loaded)
end
