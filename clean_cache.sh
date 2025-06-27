#!/bin/bash

# Remove sled DB and metadata for OneDrive sync

set -e

CACHE_DIR="$HOME/.onedrive"
SLED_DB_DIR="$CACHE_DIR/metadata.sled"
METADATA_FILE="$CACHE_DIR/metadata.json"
SETTINGS_FILE="$CACHE_DIR/settings.json"

echo "Cleaning OneDrive cache and metadata..."

if [ -d "$SLED_DB_DIR" ]; then
	echo "Removing sled DB directory: $SLED_DB_DIR"
	rm -rf "$SLED_DB_DIR"
else
	echo "No sled DB directory found at $SLED_DB_DIR"
fi

if [ -f "$METADATA_FILE" ]; then
    	echo "Removing metadata file: $METADATA_FILE"
	rm -f "$METADATA_FILE"
else
    	echo "No metadata file found at $METADATA_FILE"
fi

# Optionally, remove settings file if you want a full reset
# Uncomment the following lines if you want to remove settings as well
# if [ -f "$SETTINGS_FILE" ]; then
#     echo "Removing settings file: $SETTINGS_FILE"
#     rm -f "$SETTINGS_FILE"
# else
#     echo "No settings file found at $SETTINGS_FILE"
# fi

echo "Cache and metadata cleanup complete."
