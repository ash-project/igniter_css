defmodule IgniterCss.Application do
  # See https://hexdocs.pm/elixir/Application.html
  # for more information on OTP Applications
  @moduledoc false

  use Application

  @impl true
  def start(_type, _args) do
    Application.ensure_all_started(:pythonx)
    wheel_path = Application.app_dir(:igniter_css, "priv/python/css_tools-0.1.0-py3-none-any.whl")

    # Set configuration directly
    pyproject_toml = """
    [project]
    name = "igniter_py"
    version = "0.4.4"
    requires-python = "==3.13.*"
    dependencies = [
      "tinycss2==1.4.0",
      "css_tools==0.1.0"
    ]
    [tool.uv.sources]
    css_tools = { path = "#{wheel_path}" }
    """

    Pythonx.uv_init(pyproject_toml)

    children = []

    # See https://hexdocs.pm/elixir/Supervisor.html
    # for other strategies and supported options
    opts = [strategy: :one_for_one, name: IgniterCss.Supervisor]
    Supervisor.start_link(children, opts)
  end
end
