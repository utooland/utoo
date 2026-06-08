#!/bin/bash
# Without this, a failed npm publish is masked by the trailing cat/rm.
set -euo pipefail

# args check
if [ "$#" -lt 5 ] || [ "$#" -gt 7 ]; then
    echo "Usage: $0 <package-name> <version> <binary-path> <os> <cpu> [tag] [--dry-run]"
    echo "  tag: npm dist-tag (default: latest, or prerelease identifier for prerelease versions)"
    exit 1
fi

NAME=$1
VERSION=$2
BINARY=$3
OS=$4
CPU=$5
NPM_TAG=""
DRY_RUN=""

default_npm_tag() {
  local version=$1
  if [[ "$version" == *"-"* ]]; then
    local prerelease="${version#*-}"
    echo "${prerelease%%.*}"
  else
    echo "latest"
  fi
}

for ARG in "${@:6}"; do
  if [ "$ARG" = "--dry-run" ]; then
    DRY_RUN="--dry-run"
  elif [ -z "$NPM_TAG" ]; then
    NPM_TAG="$ARG"
  else
    echo "Unexpected argument: $ARG" >&2
    exit 1
  fi
done

if [ -z "$NPM_TAG" ]; then
  NPM_TAG=$(default_npm_tag "$VERSION")
fi

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

# do publish; pass --dry-run for verification without publishing
cd "$PLATFORM_DIR"
echo "Publishing @utoo/$NAME-$OS-$CPU@$VERSION with tag: $NPM_TAG"
if [ "$DRY_RUN" = "--dry-run" ]; then
  npm publish --provenance --access public --tag "$NPM_TAG" --dry-run
else
  npm publish --provenance --access public --tag "$NPM_TAG"
fi
cat package.json

# clean up
rm -rf "$WORK_DIR"
