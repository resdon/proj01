#!/bin/bash
# Usage: ./open_with.sh <executable> <file_path>
EXECUTABLE=$1
FILE_PATH=$2

# Launch the app with the file path
"$EXECUTABLE" "$FILE_PATH" &
