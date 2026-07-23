#!/bin/bash
# Without this, a failed npm publish is masked by the trailing cat/rm.
set -euo pipefail

# args check
if [ "$#" -lt 1 ] || [ "$#" -gt 3 ]; then
    echo "Usage: $0 <version> [tag] [--dry-run]"
    echo "  version: npm package version (e.g., 1.0.0, 1.0.0-beta)"
    echo "  tag: npm dist-tag (default: latest, or prerelease identifier for prerelease versions)"
    exit 1
fi

VERSION=$1
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

for ARG in "${@:2}"; do
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

# create temp dir
WORK_DIR=$(mktemp -d)
echo "Working in temporary directory: $WORK_DIR"

# create vendor dir
ENTRY_DIR="$WORK_DIR/entry"
mkdir -p "$ENTRY_DIR"

# render utoo package.json template
cp ../templates/utoo.package.json.template "$ENTRY_DIR/package.json"
cat "$ENTRY_DIR/package.json" | \
    awk -v version="$VERSION" \
    '{
        gsub(/{{version}}/, version);
        print;
    }' > "$ENTRY_DIR/package.json.tmp" && mv "$ENTRY_DIR/package.json.tmp" "$ENTRY_DIR/package.json"

# copy README.md from repository root
cp ../../README.md "$ENTRY_DIR/README.md"

# Immutable runtime launchers. Package managers install the matching optional
# platform artifact; these files only locate and spawn it. No lifecycle hook or
# mutation of package-manager-owned directories is required.
mkdir -p "$ENTRY_DIR/bin"
cp ../templates/launcher.utoo.js.template "$ENTRY_DIR/bin/launcher.js"
cp ../templates/utoo.utoo.js.template "$ENTRY_DIR/bin/utoo.js"
cp ../templates/utx.utoo.js.template "$ENTRY_DIR/bin/utx.js"
chmod +x "$ENTRY_DIR/bin/utoo.js" "$ENTRY_DIR/bin/utx.js"

# do publish
cd "$ENTRY_DIR"
echo "Publishing utoo@$VERSION with tag: $NPM_TAG"
if [ "$DRY_RUN" = "--dry-run" ]; then
  npm publish --provenance --access public --tag "$NPM_TAG" --dry-run
else
  npm publish --provenance --access public --tag "$NPM_TAG"
fi
cat package.json

# clean up temp dir
rm -rf "$WORK_DIR"
