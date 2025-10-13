# SPDX-FileCopyrightText: 2025 Shahryar Tavakkoli
#
# SPDX-License-Identifier: MIT

from setuptools import setup, find_packages
# Create a virtual environment in your project
# python3 -m venv plibs/venv

# Activate the virtual environment
# source plibs/venv/bin/activate

# Now install build and any other dependencies
# pip install build
# pip install tinycss2

# Navigate to your package directory
# cd plibs/css_tools

# Build the package
# python -m build

# Or use the rebuild script from project root:
# ./rebuild_wheel.sh
setup(
    name="css_tools",
    version="0.1.2",
    packages=find_packages(where="src"),
    package_dir={"": "src"},
    install_requires=[
        "tinycss2>=1.4.0",
    ],
)
