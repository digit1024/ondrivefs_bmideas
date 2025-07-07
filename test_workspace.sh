#!/bin/bash

echo "🔧 Testing OneDrive Sync Workspace"
echo "=================================="

# Test lib build
echo "📦 Building lib..."
if cargo check -p onedrive-sync-lib; then
    echo "✅ Lib builds successfully"
else
    echo "❌ Lib build failed"
    exit 1
fi

# Test hello-world build
echo "📦 Building hello-world app..."
if cargo check -p onedrive-sync-ui; then
    echo "✅ Hello-world app builds successfully"
else
    echo "❌ Hello-world app build failed"
    exit 1
fi

# Test daemon build (without DBus for now)
echo "📦 Building daemon (basic check)..."
if cargo check -p onedrive-sync-daemon --no-default-features; then
    echo "✅ Daemon builds successfully (basic)"
else
    echo "❌ Daemon build failed"
    exit 1
fi

echo ""
echo "🎉 Workspace test completed successfully!"
echo ""
echo "📋 Summary:"
echo "  ✅ Lib crate: Working"
echo "  ✅ Hello-world app: Working" 
echo "  ✅ Daemon: Basic build working"
echo ""
echo "💡 Next steps:"
echo "  1. Fix DBus server implementation for daemon"
echo "  2. Test actual DBus communication"
echo "  3. Integrate with existing OneDrive sync logic" 