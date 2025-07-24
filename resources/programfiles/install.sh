#!/bin/bash

# OneDrive Sync - Installation Script
# This script installs the open-onedrive handler and MIME type registration

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
DAEMON_BINARY="$PROJECT_ROOT/../target/release/onedrive-sync-daemon"
SYMLINK_PATH="$HOME/.local/bin/open-onedrive"
DESKTOP_FILE="$HOME/.local/share/applications/open-onedrive.desktop"
MIME_FILE="$HOME/.local/share/mime/packages/onedrive-sync.xml"

echo -e "${GREEN}🚀 Installing OneDrive Sync Handler...${NC}"
echo -e "${GREEN}⚙️ BUilding For release first...${NC}"
cargo build --release --manifest-path "$PROJECT_ROOT/../Cargo.toml" --target-dir "$PROJECT_ROOT/../target"
systemctl --user stop open-onedrive-daemon.service
# Check if daemon binary exists
if [ ! -f "$DAEMON_BINARY" ]; then
    echo -e "${RED}❌ Daemon binary not found at: $DAEMON_BINARY${NC}"
    echo -e "${YELLOW}💡 Please build the project first with: cargo build${NC}"
    exit 1
fi

# Create ~/.local/bin if it doesn't exist
if [ ! -d "$HOME/.local/bin" ]; then
    echo -e "${YELLOW}📁 Creating ~/.local/bin directory...${NC}"
    mkdir -p "$HOME/.local/bin"
fi

# Create symlink
echo -e "${YELLOW}🔗 Creating symlink: $SYMLINK_PATH -> $DAEMON_BINARY${NC}"
if [ -L "$SYMLINK_PATH" ]; then
    rm "$SYMLINK_PATH"
fi
ln -sf "$DAEMON_BINARY" "$SYMLINK_PATH"
chmod +x "$SYMLINK_PATH"

# Create symlink for UI
UI_BINARY="$PROJECT_ROOT/../target/release/onedrive-sync-ui"
UI_SYMLINK_PATH="$HOME/.local/bin/onedrive-sync-ui"
if [ -f "$UI_BINARY" ]; then
    echo -e "${YELLOW}🔗 Creating symlink: $UI_SYMLINK_PATH -> $UI_BINARY${NC}"
    if [ -L "$UI_SYMLINK_PATH" ]; then
        rm "$UI_SYMLINK_PATH"
    fi
    ln -sf "$UI_BINARY" "$UI_SYMLINK_PATH"
    chmod +x "$UI_SYMLINK_PATH"
else
    echo -e "${YELLOW}⚠️ UI binary not found at: $UI_BINARY${NC}"
fi

# Create applications directory if it doesn't exist
if [ ! -d "$HOME/.local/share/applications" ]; then
    echo -e "${YELLOW}📁 Creating applications directory...${NC}"
    mkdir -p "$HOME/.local/share/applications"
fi

# Install desktop file for daemon/file handler
DAEMON_DESKTOP_FILE="$HOME/.local/share/applications/open-onedrive-daemon.desktop"
echo -e "${YELLOW}🔗 Creating symlink: $DAEMON_DESKTOP_FILE -> $DAEMON_DESKTOP_FILE${NC}"
cp "$SCRIPT_DIR/open-onedrive-daemon.desktop" "$DAEMON_DESKTOP_FILE"

# Install desktop file for UI
UI_DESKTOP_FILE="$HOME/.local/share/applications/open-onedrive-ui.desktop"
echo -e "${YELLOW}📄 Installing UI desktop file...${NC}"
cp "$SCRIPT_DIR/open-onedrive-ui.desktop" "$UI_DESKTOP_FILE"

# Install desktop file for applet
APPLET_DESKTOP_FILE="$HOME/.local/share/applications/open-onedrive-applet.desktop"
echo -e "${YELLOW}📄 Installing Applet desktop file...${NC}"
cp "$SCRIPT_DIR/open-onedrive-applet.desktop" "$APPLET_DESKTOP_FILE"

# Create symlink for Applet binary
APPLET_BINARY="$PROJECT_ROOT/../target/release/onedrive-sync-applet"
APPLET_SYMLINK_PATH="$HOME/.local/bin/onedrive-sync-applet"
if [ -f "$APPLET_BINARY" ]; then
    echo -e "${YELLOW}🔗 Creating symlink: $APPLET_SYMLINK_PATH -> $APPLET_BINARY${NC}"
    if [ -L "$APPLET_SYMLINK_PATH" ]; then
        rm "$APPLET_SYMLINK_PATH"
    fi
    ln -sf "$APPLET_BINARY" "$APPLET_SYMLINK_PATH"
    chmod +x "$APPLET_SYMLINK_PATH"
else
    echo -e "${YELLOW}⚠️ Applet binary not found at: $APPLET_BINARY${NC}"
fi

# Create mime directory if it doesn't exist
if [ ! -d "$HOME/.local/share/mime/packages" ]; then
    echo -e "${YELLOW}📁 Creating MIME packages directory...${NC}"
    mkdir -p "$HOME/.local/share/mime/packages"
fi

# Install MIME type definition
echo -e "${YELLOW}📄 Installing MIME type definition...${NC}"
cp "$SCRIPT_DIR/onedrive-sync.xml" "$MIME_FILE"

# Update desktop and MIME databases
echo -e "${YELLOW}🔄 Updating desktop database...${NC}"
update-desktop-database "$HOME/.local/share/applications"

echo -e "${YELLOW}🔄 Updating MIME database...${NC}"
update-mime-database "$HOME/.local/share/mime"

# Icon install with multiple sizes
ICON_SIZES="16 32 48 64 128 256"
ICONS_DIR="$SCRIPT_DIR/icons"

if [ -d "$ICONS_DIR" ]; then
    echo -e "${YELLOW}🖼️ Installing all application icons from $ICONS_DIR...${NC}"
    for icon_file in "$ICONS_DIR"/*; do
        icon_name=$(basename "$icon_file")
        for size in $ICON_SIZES; do
            ICON_DST="$HOME/.local/share/icons/hicolor/${size}x${size}/apps/open-onedrive-$icon_name"
            mkdir -p "$(dirname "$ICON_DST")"
            cp "$icon_file" "$ICON_DST"
            echo -e "   • ${size}x${size} open-onedrive-$icon_name installed"
        done
    done
else
    echo -e "${YELLOW}⚠️ Icons directory not found: $ICONS_DIR${NC}"
fi

# Also install open-onedrive.png
DOWNLOADING_SRC="$SCRIPT_DIR/open-onedrive.png"
if [ -f "$DOWNLOADING_SRC" ]; then
    for size in $ICON_SIZES; do
        DOWNLOADING_DST="$HOME/.local/share/icons/hicolor/${size}x${size}/apps/open-onedrive.png"
        mkdir -p "$(dirname "$DOWNLOADING_DST")"
        cp "$DOWNLOADING_SRC" "$DOWNLOADING_DST"
        echo -e "   • ${size}x${size} downloading icon installed"
    done
else
    echo -e "${YELLOW}⚠️ Downloading icon file not found: $DOWNLOADING_SRC${NC}"
fi

# Update icon cache
# if command -v gtk-update-icon-cache >/dev/null 2>&1; then
#     echo -e "${YELLOW}🔄 Updating icon cache...${NC}"
#     gtk-update-icon-cache "$HOME/.local/share/icons/hicolor"
# fi

echo -e "${GREEN}✅ Installation completed successfully!${NC}"
echo -e "${GREEN}📋 Summary:${NC}"
echo -e "   • Symlink: $SYMLINK_PATH"
echo -e "   • Desktop file: $DAEMON_DESKTOP_FILE"
echo -e "   • UI Desktop file: $UI_DESKTOP_FILE"
echo -e "   • MIME type: application/onedrivedownload"
echo -e "   • MIME file: $MIME_FILE"
echo -e "   • Icon: $ICON_DST"
echo -e "   • UI Symlink: $UI_SYMLINK_PATH"
echo -e "   • Applet Desktop file: $APPLET_DESKTOP_FILE"
echo -e "   • Applet Symlink: $APPLET_SYMLINK_PATH"
echo -e ""
echo -e "${YELLOW}💡 To test:${NC}"
echo -e "   • Open a file with MIME type application/onedrivedownload"
echo -e "   • It should open in your OneDrive Sync application"
echo -e ""
echo -e "${YELLOW}💡 To uninstall:${NC}"
echo -e "   • Run: $SCRIPT_DIR/uninstall.sh" 

echo "STARTING DAEMON NOW!"
systemctl --user start open-onedrive-daemon.service
