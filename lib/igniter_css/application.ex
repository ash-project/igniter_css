# SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
#
# SPDX-License-Identifier: MIT

defmodule IgniterCss.Application do
  # See https://hexdocs.pm/elixir/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    # Nothing to boot: the parser is a precompiled NIF loaded on first use.
    # There is no interpreter to initialise and no external process to start.
    Supervisor.start_link([], strategy: :one_for_one, name: IgniterCss.Supervisor)
  end
end
