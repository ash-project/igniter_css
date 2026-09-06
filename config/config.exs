# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

import Config

if Mix.env() != :prod do
  # A clone is always ahead of the last published release, so the artifact this
  # version would download does not exist yet. Build the crate instead. Only
  # applies when igniter_css is the root project: Mix does not read a
  # dependency's config, and `config/` is not in the package's `files:`.
  config :rustler_precompiled, :force_build, igniter_css: true
end

if Mix.env() == :dev do
  config :git_ops,
    mix_project: IgniterCss.MixProject,
    changelog_file: "CHANGELOG.md",
    repository_url: "https://github.com/ash-project/igniter_css",
    manage_mix_version?: true,
    manage_readme_version: [
      "README.md"
    ],
    version_tag_prefix: "v"
end
