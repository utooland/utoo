#!/bin/bash

# args check
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Usage: $0 <version> [tag]"
    echo "  version: npm package version (e.g., 1.0.0, 1.0.0-beta)"
    echo "  tag: npm dist-tag (default: latest)"
    exit 1
fi

VERSION=$1
NPM_TAG=${2:-latest}

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

# copy postinstall script (Node.js for cross-platform compatibility)
cp ../templates/postinstall.js.template "$ENTRY_DIR/postinstall.js"

# copy README.md from repository root
cp ../../README.md "$ENTRY_DIR/README.md"

# create placeholder binaries
mkdir -p "$ENTRY_DIR/bin"
for binary in utoo ut; do
    # Unix version (use /bin/sh for portability)
    cat > "$ENTRY_DIR/bin/$binary" << 'EOF'
#!/bin/sh
echo "This is a placeholder binary. The actual binary will be installed via postinstall script."
exit 1
EOF
    chmod +x "$ENTRY_DIR/bin/$binary"
    
    # Windows version (.cmd)
    cat > "$ENTRY_DIR/bin/$binary.cmd" << 'EOF'
@echo off
echo This is a placeholder binary for %~n0. The actual binary will be installed via postinstall script.
exit /b 1
EOF
done

# create utx shell script that executes utoo x
cat > "$ENTRY_DIR/bin/utx" << 'EOF'
#!/bin/sh
utoo x "$@"
EOF
chmod +x "$ENTRY_DIR/bin/utx"

# Windows version utx
cat > "$ENTRY_DIR/bin/utx.cmd" << 'EOF'
@echo off
utoo x %*
EOF

# do publish
cd "$ENTRY_DIR"
echo "Publishing utoo@$VERSION with tag: $NPM_TAG"
npm publish --provenance --access public --tag "$NPM_TAG"
cat package.json

# clean up temp dir
rm -rf "$WORK_DIR"