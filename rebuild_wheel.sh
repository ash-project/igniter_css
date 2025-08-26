#!/bin/bash
# Script to rebuild the Python wheel and ensure the latest version is used

echo "🔧 Rebuilding CSS tools wheel..."

# Remove old wheel from priv/python
echo "📦 Removing old wheel..."
rm -f priv/python/css_tools-0.1.0-py3-none-any.whl

# Navigate to css_tools directory
cd plibs/css_tools

# Activate virtual environment if it exists
if [ -d "../venv" ]; then
    echo "🐍 Activating virtual environment..."
    source ../venv/bin/activate
fi

# Clean previous builds
echo "🧹 Cleaning previous builds..."
rm -rf build/ dist/ src/css_tools.egg-info/

# Build the wheel
echo "🏗️ Building new wheel..."
python -m build

# Copy the new wheel to priv/python
echo "📋 Copying wheel to priv/python..."
cp dist/css_tools-0.1.0-py3-none-any.whl ../../priv/python/

# Verify the wheel was copied
if [ -f "../../priv/python/css_tools-0.1.0-py3-none-any.whl" ]; then
    echo "✅ Wheel successfully rebuilt and deployed!"
    ls -lh ../../priv/python/css_tools-0.1.0-py3-none-any.whl
else
    echo "❌ Failed to copy wheel to priv/python"
    exit 1
fi

echo "🎉 Done!"