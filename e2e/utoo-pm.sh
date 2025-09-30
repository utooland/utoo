#!/bin/bash


set -e

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
echo "Installing dependencies for ant-design-x (next)..."
utoo deps --con 16 || { echo -e "${RED}FAIL: utoo install failed for ant-design-x (next)${NC}"; exit 1; }
rm -rf ~/.cache/nm/
utoo deps --con 32 || { echo -e "${RED}FAIL: utoo install failed for ant-design-x (next)${NC}"; exit 1; }
rm -rf ~/.cache/nm/
utoo deps --con 48 || { echo -e "${RED}FAIL: utoo install failed for ant-design-x (next)${NC}"; exit 1; }
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed for ant-design-x (next)${NC}"; exit 1; }
utoo rebuild || { echo -e "${RED}FAIL: utoo install failed for ant-design-x (next)${NC}"; exit 1; }
echo -e "${GREEN}PASS: ant-design-x (next) cloned and installed${NC}"
cd ../../

# Case 2: Clone and install ant-design
echo -e "${YELLOW}Case 2: Clone and install ant-design${NC}"
cd ant-design
if [ ! -d "ant-design" ]; then
  git clone --depth=1 --single-branch https://github.com/ant-design/ant-design.git
fi
cd ant-design
echo "Installing dependencies for ant-design..."
utoo install || { echo -e "${RED}FAIL: utoo install failed for ant-design${NC}"; exit 1; }
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


# Case 8: reinstall ant-design
echo -e "${YELLOW}Case 8: Clone and install ant-design${NC} by npmjs.org"
cd e2e/pm/ant-design
git clean -dfx
echo "Installing dependencies for ant-design by npmjs.org..."
utoo install --registry=https://registry.npmjs.org || { echo -e "${RED}FAIL: utoo install failed for ant-design${NC}"; exit 1; }
echo -e "${GREEN}PASS: ant-design cloned and installed${NC}"
cd ../../

echo -e "${GREEN}All e2e tests passed successfully!${NC}"
