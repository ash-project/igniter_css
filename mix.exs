# SPDX-FileCopyrightText: 2025 Shahryar Tavakkoli
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.MixProject do
  use Mix.Project
  @version "0.1.1"
  @source_url "https://github.com/ash-project/igniter_css"

  @description """
  CSS codemods, powered by a Python parser integrated via NIFs
  """

  def project do
    [
      app: :igniter_css,
      version: @version,
      elixir: "~> 1.17",
      package: package(),
      aliases: aliases(),
      elixirc_paths: elixirc_paths(Mix.env()),
      start_permanent: Mix.env() == :prod,
      docs: docs(),
      deps: deps(),
      description: @description,
      package: package(),
      source_url: @source_url,
      homepage_url: @source_url
    ]
  end

  defp package() do
    [
      files: ~w[
          lib
          priv
          .formatter.exs
          mix.exs
          LICENSE
          README*
        ],
      maintainers: [
        "Zach Daniel <zach@zachdaniel.dev",
        "Shahryar Tavakkoli <shahryar.tbiz@gmail.com>"
      ],
      licenses: ["MIT"],
      links: %{
        "GitHub" => @source_url,
        "Changelog" => "#{@source_url}/blob/main/CHANGELOG.md",
        "Discord" => "https://discord.gg/HTHRaaVPUc",
        "Website" => "https://ash-hq.org",
        "Forum" => "https://elixirforum.com/c/ash-framework-forum/",
        "REUSE Compliance" => "https://api.reuse.software/info/github.com/ash-project/igniter_css"
      }
    ]
  end

  defp elixirc_paths(:test), do: ["lib", "test/support"]
  defp elixirc_paths(_), do: ["lib"]

  defp docs do
    [
      main: "readme",
      source_ref: "v#{@version}",
      logo: "logos/igniter-logo.png",
      extra_section: "GUIDES",
      before_closing_head_tag: fn type ->
        if type == :html do
          """
          <script>
            if (location.hostname === "hexdocs.pm") {
              var script = document.createElement("script");
              script.src = "https://plausible.io/js/script.js";
              script.setAttribute("defer", "defer")
              script.setAttribute("data-domain", "ashhexdocs")
              document.head.appendChild(script);
            }
          </script>
          """
        end
      end,
      extras: [
        {"README.md", title: "Home"},
        "CHANGELOG.md"
      ],
      groups_for_extras: [
        Tutorials: ~r'documentation/tutorials',
        "How To": ~r'documentation/how_to',
        Topics: ~r'documentation/topics',
        DSLs: ~r'documentation/dsls',
        "About IgniterCss": [
          "CHANGELOG.md"
        ]
      ]
      # groups_for_modules: [
      # ]
    ]
  end

  # Run "mix help compile.app" to learn about applications.
  def application do
    [
      extra_applications: [:logger],
      mod: {IgniterCss.Application, []}
    ]
  end

  # Run "mix help deps" to learn about dependencies.
  defp deps do
    [
      {:pythonx, "~> 0.4"},
      {:rustler, ">= 0.0.0", optional: true},
      {:igniter_js, "~> 0.4.6", optional: true},
      {:mix_audit, ">= 0.0.0", only: [:dev, :test], runtime: false},
      {:sobelow, ">= 0.0.0", only: [:dev, :test], runtime: false},
      {:dialyxir, ">= 0.0.0", only: [:dev, :test], runtime: false},
      {:ex_check, "~> 0.16", only: [:dev, :test]},
      {:credo, ">= 0.0.0", only: [:dev, :test], runtime: false},
      {:ex_doc, "~> 0.38", only: [:dev, :test], runtime: false}
    ]
  end

  defp aliases do
    [
      sobelow: "sobelow --skip",
      credo: "credo --strict"
    ]
  end
end
