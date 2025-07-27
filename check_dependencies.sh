#!/bin/bash

echo "🔍 Checking system dependencies for onedrive-sync..."

# Core dependencies
echo "📦 Core dependencies:"
pkg-config --exists fuse3 2>/dev/null && echo "✅ libfuse-dev" || echo "❌ libfuse-dev (missing)"
pkg-config --exists openssl 2>/dev/null && echo "✅ libssl-dev" || echo "❌ libssl-dev (missing)"
pkg-config --exists dbus-1 2>/dev/null && echo "✅ libdbus-1-dev" || echo "❌ libdbus-1-dev (missing)"

# UI dependencies
echo "🖥️  UI dependencies:"
pkg-config --exists gtk+-3.0 2>/dev/null && echo "✅ libgtk-3-dev" || echo "❌ libgtk-3-dev (missing)"
pkg-config --exists webkit2gtk-4.0 2>/dev/null && echo "✅ libwebkit2gtk-4.0-dev" || echo "❌ libwebkit2gtk-4.0-dev (missing)"
pkg-config --exists ayatana-appindicator3-0.1 2>/dev/null && echo "✅ libayatana-appindicator3-dev" || echo "❌ libayatana-appindicator3-dev (missing)"

# Check if binaries can be built
echo "🔨 Build test:"
if cargo check --quiet 2>/dev/null; then
    echo "✅ All Rust dependencies satisfied"
else
    echo "❌ Missing some dependencies - run 'cargo check' for details"
fi

echo ""
echo "💡 To install missing dependencies:"
echo "sudo apt install libfuse-dev libssl-dev libdbus-1-dev libgtk-3-dev libwebkit2gtk-4.0-dev libayatana-appindicator3-dev pkg-config build-essential" 