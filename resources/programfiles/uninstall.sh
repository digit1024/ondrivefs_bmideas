#!/bin/bash

# OneDrive Sync - Uninstallation Script
# This script removes the open-onedrive handler and MIME type registration

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SYMLINK_PATH="$HOME/.local/bin/open-onedrive"
DESKTOP_FILE="$HOME/.local/share/applications/open-onedrive.desktop"
MIME_FILE="$HOME/.local/share/mime/packages/onedrive-sync.xml"

echo -e "${GREEN}🗑️ Uninstalling OneDrive Sync Handler...${NC}"

# Remove symlink
if [ -L "$SYMLINK_PATH" ]; then
    echo -e "${YELLOW}🔗 Removing symlink: $SYMLINK_PATH${NC}"
    rm "$SYMLINK_PATH"
else
    echo -e "${YELLOW}ℹ️ Symlink not found: $SYMLINK_PATH${NC}"
fi

# Remove desktop file for daemon/file handler
DAEMON_DESKTOP_FILE="$HOME/.local/share/applications/open-onedrive-daemon.desktop"
if [ -f "$DAEMON_DESKTOP_FILE" ]; then
    echo -e "${YELLOW}🔗 Removing daemon desktop file: $DAEMON_DESKTOP_FILE${NC}"
    rm "$DAEMON_DESKTOP_FILE"
else
    echo -e "${YELLOW}ℹ️ Daemon desktop file not found: $DAEMON_DESKTOP_FILE${NC}"
fi

# Remove desktop file for UI
UI_DESKTOP_FILE="$HOME/.local/share/applications/open-onedrive-ui.desktop"
if [ -f "$UI_DESKTOP_FILE" ]; then
    echo -e "${YELLOW}📄 Removing UI desktop file: $UI_DESKTOP_FILE${NC}"
    rm "$UI_DESKTOP_FILE"
else
    echo -e "${YELLOW}ℹ️ UI desktop file not found: $UI_DESKTOP_FILE${NC}"
fi

# Remove MIME type definition
if [ -f "$MIME_FILE" ]; then
    echo -e "${YELLOW}📄 Removing MIME type definition: $MIME_FILE${NC}"
    rm "$MIME_FILE"
else
    echo -e "${YELLOW}ℹ️ MIME file not found: $MIME_FILE${NC}"
fi

# Remove SVG icon from scalable folder
SVG_ICON="$HOME/.local/share/icons/hicolor/scalable/apps/open-onedrive.svg"
if [ -f "$SVG_ICON" ]; then
    echo -e "${YELLOW}🖼️ Removing SVG icon: $SVG_ICON${NC}"
    rm "$SVG_ICON"
    ICONS_REMOVED=1
else
    echo -e "${YELLOW}ℹ️ SVG icon not found: $SVG_ICON${NC}"
    ICONS_REMOVED=0
fi

if [ $ICONS_REMOVED -gt 0 ]; then
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        echo -e "${YELLOW}🔄 Updating icon cache...${NC}"
        gtk-update-icon-cache "$HOME/.local/share/icons/hicolor"
    fi
else
    echo -e "${YELLOW}ℹ️ No icons found to remove${NC}"
fi

echo -e "   • Icon: $ICON_DST"

# Update desktop and MIME databases
echo -e "${YELLOW}🔄 Updating desktop database...${NC}"
if [ -d "$HOME/.local/share/applications" ]; then
    update-desktop-database "$HOME/.local/share/applications"
fi

echo -e "${YELLOW}🔄 Updating MIME database...${NC}"
if [ -d "$HOME/.local/share/mime" ]; then
    update-mime-database "$HOME/.local/share/mime"
fi

# Remove symlink for UI
UI_SYMLINK_PATH="$HOME/.local/bin/onedrive-sync-ui"
if [ -L "$UI_SYMLINK_PATH" ]; then
    echo -e "${YELLOW}🔗 Removing UI symlink: $UI_SYMLINK_PATH${NC}"
    rm "$UI_SYMLINK_PATH"
else
    echo -e "${YELLOW}ℹ️ UI symlink not found: $UI_SYMLINK_PATH${NC}"
fi

echo -e "   • UI Symlink: $UI_SYMLINK_PATH"

echo -e "   • Daemon Desktop file: $DAEMON_DESKTOP_FILE"

echo -e "${GREEN}✅ Uninstallation completed successfully!${NC}"
echo -e "${GREEN}📋 Removed:${NC}"
echo -e "   • Symlink: $SYMLINK_PATH"
echo -e "   • Desktop file: $DESKTOP_FILE"
echo -e "   • UI Desktop file: $UI_DESKTOP_FILE"
echo -e "   • MIME type: application/onedrivedownload"
echo -e "   • MIME file: $MIME_FILE"
echo -e "   • Icons: $ICONS_REMOVED icon files removed" 