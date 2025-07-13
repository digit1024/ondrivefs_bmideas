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

# Remove desktop file
if [ -f "$DESKTOP_FILE" ]; then
    echo -e "${YELLOW}📄 Removing desktop file: $DESKTOP_FILE${NC}"
    rm "$DESKTOP_FILE"
else
    echo -e "${YELLOW}ℹ️ Desktop file not found: $DESKTOP_FILE${NC}"
fi

# Remove MIME type definition
if [ -f "$MIME_FILE" ]; then
    echo -e "${YELLOW}📄 Removing MIME type definition: $MIME_FILE${NC}"
    rm "$MIME_FILE"
else
    echo -e "${YELLOW}ℹ️ MIME file not found: $MIME_FILE${NC}"
fi

# Remove icons in all sizes
ICON_SIZES="16 32 48 64 128 256"
ICONS_REMOVED=0

for size in $ICON_SIZES; do
    ICON_DST="$HOME/.local/share/icons/hicolor/${size}x${size}/apps/open-onedrive.png"
    if [ -f "$ICON_DST" ]; then
        echo -e "${YELLOW}🖼️ Removing ${size}x${size} icon: $ICON_DST${NC}"
        rm "$ICON_DST"
        ICONS_REMOVED=$((ICONS_REMOVED + 1))
    fi
done

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

echo -e "${GREEN}✅ Uninstallation completed successfully!${NC}"
echo -e "${GREEN}📋 Removed:${NC}"
echo -e "   • Symlink: $SYMLINK_PATH"
echo -e "   • Desktop file: $DESKTOP_FILE"
echo -e "   • MIME type: application/onedrivedownload"
echo -e "   • MIME file: $MIME_FILE"
echo -e "   • Icons: $ICONS_REMOVED icon files removed" 