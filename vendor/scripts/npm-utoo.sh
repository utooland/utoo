#!/bin/bash

# args check
if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <version>"
    exit 1
fi

VERSION=$1

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

# do copy postinstall.sh
cp ../templates/postinstall.utoo.sh.template "$ENTRY_DIR/postinstall.sh"
chmod +x "$ENTRY_DIR/postinstall.sh"

# copy README.md from repository root
cp ../../README.md "$ENTRY_DIR/README.md"

# create placeholder binaries
mkdir -p "$ENTRY_DIR/bin"
for binary in utoo ut; do
    # Unix 版本
    cat > "$ENTRY_DIR/bin/$binary" << 'EOF'
#!/bin/bash
echo "This is a placeholder binary for $binary. The actual binary will be installed via postinstall script."
exit 1
EOF
    chmod +x "$ENTRY_DIR/bin/$binary"
    
    # Windows 版本 (.cmd)
    cat > "$ENTRY_DIR/bin/$binary.cmd" << 'EOF'
@echo off
echo This is a placeholder binary for %~n0. The actual binary will be installed via postinstall script.
exit /b 1
EOF
done

# create utx shell script that executes utoo x
cat > "$ENTRY_DIR/bin/utx" << 'EOF'
#!/bin/bash
utoo x "$@"
EOF
chmod +x "$ENTRY_DIR/bin/utx"

# Windows 版本的 utx
cat > "$ENTRY_DIR/bin/utx.cmd" << 'EOF'
@echo off
utoo x %*
EOF

# do publish
cd "$ENTRY_DIR"
npm publish --provenance --access public
cat package.json

# clean up temp dir
rm -rf "$WORK_DIR"