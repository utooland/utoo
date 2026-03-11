#!/bin/bash

set -e
set -o pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}Starting utoo-pm e2e tests...${NC}"
echo -e "utoo path: $(which utoo)"
echo -e "ut path: $(which ut)"
echo -e "node path: $(node -e 'console.log(process.arch)')"

ut config set registry https://registry.npmjs.org --global

# Case 1: Clone and install ant-design-x (next)
echo -e "${YELLOW}Case 1: Clone and install ant-design-x (next)${NC}"
cd e2e/pm/ant-design-x
if [ ! -d "ant-design-x" ]; then
  git clone --branch next --single-branch https://github.com/ant-design/x.git ant-design-x
fi
cd ant-design-x

rm -rf node_modules package-lock.json
rm -rf ~/.cache/nm
time utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed for ant-design-x${NC}"; exit 1; }

utoo rebuild || { echo -e "${RED}FAIL: utoo rebuild failed for ant-design-x (next)${NC}"; exit 1; }
echo -e "${GREEN}PASS: ant-design-x (next) cloned and installed${NC}"
cd ../../

# Case 2: Clone and install ant-design
echo -e "${YELLOW}Case 2: Clone and install ant-design${NC}"
cd ant-design
if [ ! -d "ant-design" ]; then
  git clone --depth=1 --single-branch https://github.com/ant-design/ant-design.git
fi
cd ant-design
rm -rf ~/.cache/nm
echo "Installing dependencies for ant-design..."
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed for ant-design${NC}"; exit 1; }
echo -e "${GREEN}PASS: ant-design cloned and installed${NC}"
cd ../../

# Case 3: antd-test project install
echo -e "${YELLOW}Case 3: antd-test project install${NC}"
cd antd-test
utoo install
if [ ! -d "node_modules" ]; then
    echo -e "${RED}FAIL: node_modules directory not created${NC}"
    exit 1
fi
if [ ! -d "node_modules/antd" ]; then
    echo -e "${RED}FAIL: antd package not installed${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: antd-test install successful${NC}"
cd ..

# Case 4: local-package link test
echo -e "${YELLOW}Case 4: local-package link test${NC}"
cd local-package
utoo install
utoo link
echo -e "${GREEN}PASS: local-package link successful${NC}"
cd ..

# Case 5: antd-test secondary install
echo -e "${YELLOW}Case 5: antd-test secondary install${NC}"
cd antd-test
utoo install
if [ ! -d "node_modules/lodash" ]; then
    echo -e "${RED}FAIL: lodash package not installed in secondary update${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: antd-test secondary install successful${NC}"
cd ..

# Case 6: antd-test deps tree
echo -e "${YELLOW}Case 6: antd-test deps tree${NC}"
cd antd-test
utoo deps
if [ ! -f "package-lock.json" ]; then
    echo -e "${RED}FAIL: utoo deps did not generate output${NC}"
    exit 1
fi
if ! grep -q "antd" package-lock.json; then
    echo -e "${RED}FAIL: utoo deps output does not contain antd${NC}"
    exit 1
fi
if ! grep -q "react" package-lock.json; then
    echo -e "${RED}FAIL: utoo deps output does not contain react${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: antd-test deps tree successful${NC}"
cd ../../..


# Case 7: test global install
echo -e "${YELLOW}Case 7: cowsay global install/uninstall${NC}"

# Test global install
utoo install -g cowsay || { echo -e "${RED}FAIL: global install cowsay failed${NC}"; exit 1; }
if ! which cowsay >/dev/null 2>&1; then
    echo -e "${RED}FAIL: cowsay not found in PATH after global install${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: cowsay global install successful${NC}"


# Case 8: git dependency install
echo -e "${YELLOW}Case 8: git dependency install${NC}"
cd e2e/pm/git-deps
rm -rf node_modules package-lock.json
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed for git-deps${NC}"; exit 1; }
if [ ! -d "node_modules" ]; then
    echo -e "${RED}FAIL: node_modules directory not created${NC}"
    exit 1
fi
# github:owner/repo shorthand
if [ ! -d "node_modules/abbrev" ]; then
    echo -e "${RED}FAIL: abbrev (github: shorthand) not installed${NC}"
    exit 1
fi
# git+https:// with tag ref
if [ ! -d "node_modules/ini" ]; then
    echo -e "${RED}FAIL: ini (git+https with tag) not installed${NC}"
    exit 1
fi
# bare owner/repo shorthand with tag
if [ ! -d "node_modules/isexe" ]; then
    echo -e "${RED}FAIL: isexe (bare github shorthand) not installed${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: git dependency install successful${NC}"

# Case 8.1: git dependency warm install (cache hit)
echo -e "${YELLOW}Case 8.1: git dependency warm install${NC}"
rm -rf node_modules package-lock.json
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo warm install failed for git-deps${NC}"; exit 1; }
if [ ! -d "node_modules/abbrev" ] || [ ! -d "node_modules/ini" ] || [ ! -d "node_modules/isexe" ]; then
    echo -e "${RED}FAIL: git deps missing after warm install${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: git dependency warm install successful${NC}"
cd ../../..

# Case 9: reinstall ant-design by npmjs.org
echo -e "${YELLOW}Case 9: reinstall ant-design${NC} by npmjs.org"
cd e2e/pm/ant-design/ant-design
git clean -dfx
echo "Installing dependencies for ant-design by npmjs.org..."
utoo install --registry=https://registry.npmjs.org || { echo -e "${RED}FAIL: utoo install failed for ant-design${NC}"; exit 1; }
echo -e "${GREEN}PASS: ant-design cloned and installed${NC}"
cd ../../../../

# Case 10: catalog protocol test
echo -e "${YELLOW}Case 10: catalog protocol test${NC}"
cd e2e/pm/catalog-test
rm -rf node_modules package-lock.json packages/*/node_modules
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed for catalog-test${NC}"; exit 1; }

# Verify root dependencies resolved from catalog
if [ ! -d "node_modules/lodash" ]; then
    echo -e "${RED}FAIL: lodash not installed (catalog: default)${NC}"
    exit 1
fi
if [ ! -d "node_modules/typescript" ]; then
    echo -e "${RED}FAIL: typescript not installed (catalog:default)${NC}"
    exit 1
fi

# Verify package-lock.json was created and contains resolved versions (not catalog: refs)
if ! grep -q '"lodash"' package-lock.json; then
    echo -e "${RED}FAIL: lodash not in package-lock.json${NC}"
    exit 1
fi
if grep -q '"catalog:' package-lock.json; then
    echo -e "${RED}FAIL: unresolved catalog: specs found in package-lock.json${NC}"
    exit 1
fi

echo -e "${GREEN}PASS: catalog protocol basic install successful${NC}"

# --- Catalog update flow ---
# Update default catalog: pin lodash to exact 4.17.20
echo -e "${YELLOW}Case 10b: catalog update flow${NC}"
cp .utoo.toml .utoo.toml.bak

cat > .utoo.toml <<'EOF'
[catalog]
lodash = "^4.17.0"
debug = "^4.3.4"
typescript = "^5.0.0"

[catalogs.legacy]
debug = "^3.2.7"
EOF

rm -rf node_modules package-lock.json packages/*/node_modules
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed after catalog update${NC}"; mv .utoo.toml.bak .utoo.toml; exit 1; }

# Verify lockfile has the updated lodash spec from catalog
LODASH_SPEC=$(node -e "const lock=require('./package-lock.json'); console.log(lock.packages[''].dependencies.lodash)")
if [ "$LODASH_SPEC" != "^4.17.0" ]; then
    echo -e "${RED}FAIL: expected lodash ^4.17.0 in lockfile after catalog update, got $LODASH_SPEC${NC}"
    mv .utoo.toml.bak .utoo.toml
    exit 1
fi

# Verify named catalog (legacy) debug resolved to ^3.2.7 in lockfile
UTILS_DEBUG_SPEC=$(node -e "const lock=require('./package-lock.json'); console.log(lock.packages['packages/utils'].dependencies.debug)")
if [ "$UTILS_DEBUG_SPEC" != "^3.2.7" ]; then
    echo -e "${RED}FAIL: expected debug ^3.2.7 for catalogs.legacy in lockfile, got $UTILS_DEBUG_SPEC${NC}"
    mv .utoo.toml.bak .utoo.toml
    exit 1
fi

# Update named catalog: change legacy debug to ^4.3.4
cat > .utoo.toml <<'EOF'
[catalog]
lodash = "^4.17.0"
debug = "^4.3.4"
typescript = "^5.0.0"

[catalogs.legacy]
debug = "^4.3.4"
EOF

rm -rf node_modules package-lock.json packages/*/node_modules
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed after named catalog update${NC}"; mv .utoo.toml.bak .utoo.toml; exit 1; }

# Now utils debug should be ^4.3.4 in lockfile
UTILS_DEBUG_SPEC=$(node -e "const lock=require('./package-lock.json'); console.log(lock.packages['packages/utils'].dependencies.debug)")
if [ "$UTILS_DEBUG_SPEC" != "^4.3.4" ]; then
    echo -e "${RED}FAIL: expected debug ^4.3.4 after named catalog update, got $UTILS_DEBUG_SPEC${NC}"
    mv .utoo.toml.bak .utoo.toml
    exit 1
fi

echo -e "${GREEN}PASS: catalog update flow successful${NC}"

# Restore original .utoo.toml
mv .utoo.toml.bak .utoo.toml
cd ../../..

echo -e "${GREEN}All e2e tests passed successfully!${NC}"
