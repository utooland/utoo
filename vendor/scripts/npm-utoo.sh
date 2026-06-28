#!/bin/bash
# Without this, a failed npm publish is masked by the trailing cat/rm.
set -euo pipefail

# args check
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Usage: $0 <version> [tag]"
    echo "  version: npm package version (e.g., 1.0.0, 1.0.0-beta)"
    echo "  tag: npm dist-tag (default: latest, or prerelease identifier for prerelease versions)"
    exit 1
fi

VERSION=$1

default_npm_tag() {
  local version=$1
  if [[ "$version" == *"-"* ]]; then
    local prerelease="${version#*-}"
    echo "${prerelease%%.*}"
  else
    echo "latest"
  fi
}

NPM_TAG=${2:-$(default_npm_tag "$VERSION")}

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

# Postinstall runs via `node` (not `sh`): npm executes lifecycle scripts through
# cmd.exe on Windows, where `sh` is not on PATH for a stock Node install.
cp ../templates/postinstall.utoo.js.template "$ENTRY_DIR/postinstall.js"

# copy README.md from repository root
cp ../../README.md "$ENTRY_DIR/README.md"

# Placeholder bin. The `#!/usr/bin/env node` shebang lets npm generate working
# .cmd/.ps1 shims on Windows (invoking node, not sh). The bin map points both
# `utoo` and `ut` at bin/utoo, so only one physical file is needed. postinstall
# replaces it with the native binary on the happy path; otherwise it self-heals
# on first invocation.
mkdir -p "$ENTRY_DIR/bin"
cp ../templates/placeholder.utoo.js.template "$ENTRY_DIR/bin/utoo"
chmod +x "$ENTRY_DIR/bin/utoo"

# utx → `utoo x`. Node launcher; on Windows postinstall also drops a utx.cmd
# into the prefix on the happy path.
cp ../templates/utx.utoo.js.template "$ENTRY_DIR/bin/utx"
chmod +x "$ENTRY_DIR/bin/utx"

# do publish
cd "$ENTRY_DIR"
echo "Publishing utoo@$VERSION with tag: $NPM_TAG"
npm publish --provenance --access public --tag "$NPM_TAG"
cat package.json

# clean up temp dir
rm -rf "$WORK_DIR"
