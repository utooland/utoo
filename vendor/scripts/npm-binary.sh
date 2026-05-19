#!/bin/bash
# Without this, a failed npm publish is masked by the trailing cat/rm.
set -euo pipefail

# args check
if [ "$#" -ne 5 ]; then
    echo "Usage: $0 <package-name> <version> <binary-path> <os> <cpu>"
    exit 1
fi

NAME=$1
VERSION=$2
BINARY=$3
OS=$4
CPU=$5

# create temporary dir
WORK_DIR=$(mktemp -d)
echo "Working in temporary directory: $WORK_DIR"

# create vendor dir
PLATFORM_DIR="$WORK_DIR/binary"
mkdir -p "$PLATFORM_DIR/bin"

# render binary package.json template
cat ../templates/binary.package.json.template | \
    awk -v name="$NAME" \
        -v version="$VERSION" \
        -v platform="$OS-$CPU" \
        -v os="$OS" \
        -v cpu="$CPU" \
    '{
        gsub(/{{name}}/, name);
        gsub(/{{version}}/, version);
        gsub(/{{platform}}/, platform);
        gsub(/{{os}}/, os);
        gsub(/{{cpu}}/, cpu);
        print;
    }' > "$PLATFORM_DIR/package.json"

# cp binary
cp "$BINARY" "$PLATFORM_DIR/bin/$NAME"
chmod +x "$PLATFORM_DIR/bin/$NAME"

# do publish, --dry-run for test
cd "$PLATFORM_DIR"
npm publish --provenance --access public
cat package.json

# clean up
rm -rf "$WORK_DIR"
