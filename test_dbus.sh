#!/bin/bash

echo "🧪 Testing OneDrive Sync DBus Communication"
echo "=========================================="

# Build all components
echo "🔨 Building workspace..."
cargo build

if [ $? -ne 0 ]; then
    echo "❌ Build failed"
    exit 1
fi

echo "✅ Build successful"

# Test the UI application
echo ""
echo "🎯 Testing UI application..."
cargo run -p onedrive-sync-ui

echo ""
echo "🏁 Test completed" 