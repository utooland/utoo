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

# Test global install. The tool is installed as a *dependency* of a synthetic
# root (never a root project), so it runs the install lifecycle but never
# prepare/prepublish, and its devDependencies are not installed.
utoo install -g cowsay || { echo -e "${RED}FAIL: global install cowsay failed${NC}"; exit 1; }
if ! which cowsay >/dev/null 2>&1; then
    echo -e "${RED}FAIL: cowsay not found in PATH after global install${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: cowsay global install successful${NC}"

# Coexistence: a second global install must reify ADDITIVELY into the shared
# global node_modules — installing semver must not prune cowsay.
utoo install -g semver || { echo -e "${RED}FAIL: global install semver failed${NC}"; exit 1; }
if ! which semver >/dev/null 2>&1; then
    echo -e "${RED}FAIL: semver not found in PATH after global install${NC}"
    exit 1
fi
if ! which cowsay >/dev/null 2>&1; then
    echo -e "${RED}FAIL: cowsay pruned by second global install (coexistence broken)${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: global installs coexist (additive reify)${NC}"


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

# Case 8.2: HTTP(S) tarball dependency install
echo -e "${YELLOW}Case 8.2: HTTP tarball dependency install${NC}"
cd e2e/pm/http-tarball-deps
rm -rf node_modules package-lock.json
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed for http-tarball-deps${NC}"; exit 1; }
if [ ! -d "node_modules" ]; then
    echo -e "${RED}FAIL: node_modules directory not created${NC}"
    exit 1
fi
for pkg in abbrev ini isexe; do
    if [ ! -f "node_modules/$pkg/package.json" ]; then
        echo -e "${RED}FAIL: $pkg (http tarball) not installed${NC}"
        exit 1
    fi
done
# Lockfile records the URL — not a registry version range — as resolved source
if ! grep -q '"https://registry.npmjs.org/abbrev/-/abbrev-2.0.0.tgz"' package-lock.json; then
    echo -e "${RED}FAIL: tarball URL missing from package-lock.json resolved field${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: HTTP tarball dependency install successful${NC}"

# Case 8.3: HTTP tarball warm install (cache hit, no re-download)
echo -e "${YELLOW}Case 8.3: HTTP tarball warm install${NC}"
rm -rf node_modules package-lock.json
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo warm install failed for http-tarball-deps${NC}"; exit 1; }
for pkg in abbrev ini isexe; do
    if [ ! -d "node_modules/$pkg" ]; then
        echo -e "${RED}FAIL: $pkg missing after warm install${NC}"
        exit 1
    fi
done
echo -e "${GREEN}PASS: HTTP tarball warm install successful${NC}"
cd ../../..

# Case 8.4: file: dependency install. Directory deps install as a SYMLINK
# (npm-compatible); tarball deps extract + clone from the cache slot.
echo -e "${YELLOW}Case 8.4: file: dependency install${NC}"
cd e2e/pm/file-deps
rm -rf node_modules package-lock.json
rm -rf ~/.cache/nm/local-dir-pkg ~/.cache/nm/local-tarball-pkg
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed for file-deps${NC}"; exit 1; }
for pkg in local-dir-pkg local-tarball-pkg; do
    if [ ! -f "node_modules/$pkg/package.json" ]; then
        echo -e "${RED}FAIL: $pkg (file:) not installed${NC}"
        exit 1
    fi
done
# Directory dep must be a SYMLINK (to ../local-dir), not a copy — matches npm.
if [ ! -L "node_modules/local-dir-pkg" ]; then
    echo -e "${RED}FAIL: local-dir-pkg should be a symlink to ../local-dir, got a regular directory${NC}"
    exit 1
fi
LINK_TARGET=$(readlink node_modules/local-dir-pkg)
if [ "$LINK_TARGET" != "../local-dir" ]; then
    echo -e "${RED}FAIL: local-dir-pkg symlink points to '$LINK_TARGET', expected '../local-dir'${NC}"
    exit 1
fi
# Tarball dep must be a real directory (clone from cache), not a symlink.
if [ -L "node_modules/local-tarball-pkg" ]; then
    echo -e "${RED}FAIL: local-tarball-pkg should be a real directory (clone), got a symlink${NC}"
    exit 1
fi
ACTUAL=$(node -e "console.log(require('./node_modules/local-dir-pkg/package.json').version)")
if [ "$ACTUAL" != "0.1.0" ]; then
    echo -e "${RED}FAIL: local-dir-pkg expected v0.1.0, got $ACTUAL${NC}"
    exit 1
fi
ACTUAL=$(node -e "console.log(require('./node_modules/local-tarball-pkg/package.json').version)")
if [ "$ACTUAL" != "2.3.4" ]; then
    echo -e "${RED}FAIL: local-tarball-pkg expected v2.3.4, got $ACTUAL${NC}"
    exit 1
fi
# Lockfile entries match npm's format:
#  - dir dep:     link: true + resolved: <root-relative path>  (no file: prefix)
#  - tarball dep: resolved:   file:<root-relative path>        (with file: prefix)
# Absolute paths in the lockfile would make it non-portable across machines.
node -e '
const lock = require("./package-lock.json");
const dir = lock.packages["node_modules/local-dir-pkg"] || {};
const tar = lock.packages["node_modules/local-tarball-pkg"] || {};
if (dir.link !== true) { console.error("local-dir-pkg missing link:true", dir); process.exit(1); }
if (dir.resolved !== "local-dir") { console.error("local-dir-pkg resolved expected \"local-dir\", got", dir.resolved); process.exit(1); }
if (tar.resolved !== "file:local-tarball.tgz") { console.error("local-tarball-pkg resolved expected \"file:local-tarball.tgz\", got", tar.resolved); process.exit(1); }
' || { echo -e "${RED}FAIL: lockfile entries wrong for file: deps${NC}"; exit 1; }
echo -e "${GREEN}PASS: file: dependency install successful${NC}"

# Case 8.5: file: warm install (cache hit for tarball; dir symlink rewritten)
echo -e "${YELLOW}Case 8.5: file: warm install${NC}"
rm -rf node_modules package-lock.json
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo warm install failed for file-deps${NC}"; exit 1; }
[ -L "node_modules/local-dir-pkg" ] || { echo -e "${RED}FAIL: local-dir-pkg symlink missing after warm install${NC}"; exit 1; }
[ -d "node_modules/local-tarball-pkg" ] && [ ! -L "node_modules/local-tarball-pkg" ] \
  || { echo -e "${RED}FAIL: local-tarball-pkg should be a real dir after warm install${NC}"; exit 1; }
echo -e "${GREEN}PASS: file: warm install successful${NC}"
cd ../../..

# Case 8.5b: file: tarball located OUTSIDE the project root.
# Regression: such a tarball's root-relative lockfile entry carries `..`
# (e.g. "file:../../pkgs/foo.tgz"). BFS hashes the cache slot from the
# canonical absolute path, but install re-absolutizes the `..`-laden lockfile
# path; if the slot key isn't normalized the two disagree and the clone fails
# with "file tarball cache not found". The in-tree fixture above can't catch
# this because its path has no `..`.
echo -e "${YELLOW}Case 8.5b: file: tarball outside project root${NC}"
EXT_DIR=$(mktemp -d)
mkdir -p "$EXT_DIR/src/ext-tarball-pkg"
cat > "$EXT_DIR/src/ext-tarball-pkg/package.json" <<'EOF'
{ "name": "ext-tarball-pkg", "version": "5.6.7", "main": "index.js" }
EOF
echo "module.exports = 567;" > "$EXT_DIR/src/ext-tarball-pkg/index.js"
( cd "$EXT_DIR/src/ext-tarball-pkg" && npm pack --silent >/dev/null 2>&1 \
  && mv ext-tarball-pkg-*.tgz "$EXT_DIR/ext-tarball-pkg.tgz" )
mkdir -p "$EXT_DIR/app"
cat > "$EXT_DIR/app/package.json" <<EOF
{
  "name": "ext-tarball-app",
  "version": "1.0.0",
  "private": true,
  "dependencies": { "ext-tarball-pkg": "file:$EXT_DIR/ext-tarball-pkg.tgz" }
}
EOF
rm -rf ~/.cache/nm/ext-tarball-pkg
( cd "$EXT_DIR/app" && utoo install --ignore-scripts ) \
  || { echo -e "${RED}FAIL: install failed for tarball outside project root${NC}"; rm -rf "$EXT_DIR"; exit 1; }
if [ ! -f "$EXT_DIR/app/node_modules/ext-tarball-pkg/package.json" ]; then
    echo -e "${RED}FAIL: ext-tarball-pkg not materialized into node_modules${NC}"; rm -rf "$EXT_DIR"; exit 1
fi
ACTUAL=$(node -e "console.log(require('$EXT_DIR/app/node_modules/ext-tarball-pkg/package.json').version)")
if [ "$ACTUAL" != "5.6.7" ]; then
    echo -e "${RED}FAIL: ext-tarball-pkg expected v5.6.7, got $ACTUAL${NC}"; rm -rf "$EXT_DIR"; exit 1
fi
# Lockfile must stay portable: a root-relative `file:` path with `..`.
# Check the *form*, not a substring: when cwd and the tarball spec disagree on
# a symlinked prefix (e.g. macOS /var vs /private/var) the relative path climbs
# to the filesystem root and re-descends, so it legitimately *contains* the abs
# dir as a substring while still being relative. Absolute = the part after
# `file:` starts at a root (`/` on unix, `C:` on windows).
node -e '
const lock = require("'"$EXT_DIR"'/app/package-lock.json");
const tar = lock.packages["node_modules/ext-tarball-pkg"] || {};
const r = tar.resolved || "";
if (!r.startsWith("file:")) {
  console.error("ext-tarball-pkg resolved should be a file: path, got", tar.resolved); process.exit(1);
}
const p = r.slice(5);
if (p[0] === "/" || /^[A-Za-z]:/.test(p)) {
  console.error("ext-tarball-pkg resolved should be root-relative, not absolute:", tar.resolved); process.exit(1);
}
' || { echo -e "${RED}FAIL: lockfile entry wrong for outside-root tarball${NC}"; rm -rf "$EXT_DIR"; exit 1; }
rm -rf "$EXT_DIR"
echo -e "${GREEN}PASS: file: tarball outside project root installs${NC}"

# Case 8.6: stale lockfile is detected and regenerated on `ut install`
#
# package.json declares two deps; we seed an empty lockfile. A correct
# `ut install` must notice the mismatch, re-resolve, and install both.
# Regression guard for #2576.
echo -e "${YELLOW}Case 8.6: stale lockfile detection${NC}"
cd e2e/pm/stale-lockfile
rm -rf node_modules
cat > package-lock.json <<'JSON'
{
  "name": "e2e-stale-lockfile",
  "version": "1.0.0",
  "lockfileVersion": 3,
  "requires": true,
  "packages": {
    "": {
      "name": "e2e-stale-lockfile",
      "version": "1.0.0",
      "dependencies": {}
    }
  }
}
JSON
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed on stale lockfile fixture${NC}"; exit 1; }
for pkg in abbrev ini; do
    if [ ! -f "node_modules/$pkg/package.json" ]; then
        echo -e "${RED}FAIL: $pkg not installed (stale lockfile not regenerated)${NC}"; exit 1
    fi
done
# Assert the ROOT entry's dependencies got rewritten — grep would match
# transitive node_modules/... entries even if the regen failed.
node -e '
const lock = require("./package-lock.json");
const root = (lock.packages || {})[""] || {};
const deps = root.dependencies || {};
if (!deps.abbrev || !deps.ini) {
    console.error("root deps after install:", deps);
    process.exit(1);
}
' || { echo -e "${RED}FAIL: package-lock.json root deps not regenerated${NC}"; exit 1; }
echo -e "${GREEN}PASS: stale lockfile detected + regenerated${NC}"
cd ../../..

# Case 8.7: cross-device cache handling (Linux only).
# /dev/shm is tmpfs — a separate device from the workspace disk, writable
# without sudo — so it forces a genuine cross-filesystem cache/node_modules
# split (EXDEV). Covers both paths: an explicitly configured cross-device cache
# must still install (copy fallback), and the default cache must relocate next
# to node_modules so packages can be hardlinked.
echo -e "${YELLOW}Case 8.7: cross-device cache (Linux tmpfs)${NC}"
if [ "$(uname -s)" = "Linux" ] && [ -d /dev/shm ]; then
  XDEV_SHM="/dev/shm/utoo-xdev-$$"
  XDEV_DISK="$(mktemp -d)"
  xdev_cleanup() { rm -rf "$XDEV_SHM" "$XDEV_DISK" 2>/dev/null || true; }

  # 8.7a: explicit cross-device cache-dir (cache on tmpfs, project on disk) ->
  # respected with a WARN, install falls back to copy and still succeeds.
  mkdir -p "$XDEV_DISK/proj"
  printf '{"name":"xdev-explicit","version":"1.0.0","dependencies":{"is-odd":"3.0.1"}}\n' \
    > "$XDEV_DISK/proj/package.json"
  ( cd "$XDEV_DISK/proj" && UTOO_CACHE_DIR="$XDEV_SHM/cache" utoo install ) \
    || { echo -e "${RED}FAIL: explicit cross-device cache install failed${NC}"; xdev_cleanup; exit 1; }
  [ -d "$XDEV_DISK/proj/node_modules/is-odd" ] \
    || { echo -e "${RED}FAIL: is-odd missing (explicit xdev)${NC}"; xdev_cleanup; exit 1; }
  [ -d "$XDEV_SHM/cache" ] \
    || { echo -e "${RED}FAIL: explicit cache dir not used at $XDEV_SHM/cache${NC}"; xdev_cleanup; exit 1; }
  echo -e "${GREEN}PASS: explicit cross-device cache copies, install OK${NC}"

  # 8.7b: default cache (~/.cache/nm on disk) cross-device with a project on
  # tmpfs -> cache is relocated under the project's node_modules so links work.
  mkdir -p "$XDEV_SHM/proj"
  printf '{"name":"xdev-default","version":"1.0.0","dependencies":{"is-odd":"3.0.1"}}\n' \
    > "$XDEV_SHM/proj/package.json"
  ( cd "$XDEV_SHM/proj" && env -u UTOO_CACHE_DIR utoo install ) \
    || { echo -e "${RED}FAIL: default cross-device cache install failed${NC}"; xdev_cleanup; exit 1; }
  [ -d "$XDEV_SHM/proj/node_modules/is-odd" ] \
    || { echo -e "${RED}FAIL: is-odd missing (default xdev)${NC}"; xdev_cleanup; exit 1; }
  [ -d "$XDEV_SHM/proj/node_modules/.cache/nm" ] \
    || { echo -e "${RED}FAIL: default cache not relocated to project node_modules/.cache/nm${NC}"; xdev_cleanup; exit 1; }
  echo -e "${GREEN}PASS: default cross-device cache relocated to project, install OK${NC}"

  xdev_cleanup
else
  echo -e "${YELLOW}SKIP: cross-device cache case (non-Linux or no /dev/shm)${NC}"
fi

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

# Case 10c: pm-pack rewrites workspace:/catalog: protocols in the packed manifest (#3094)
# A workspace member depends on a sibling via workspace: and on a catalog: entry.
# `utoo pm-pack` must emit a tarball whose package.json carries concrete versions,
# otherwise downstream `npm install` of the tgz fails with EUNSUPPORTEDPROTOCOL.
echo -e "${YELLOW}Case 10c: pm-pack workspace:/catalog: rewrite${NC}"
cd e2e/pm/pack-protocols/packages/foo
rm -f ./*.tgz
utoo pm-pack || { echo -e "${RED}FAIL: utoo pm-pack failed${NC}"; cd ../../../../..; exit 1; }
PACK_TGZ=$(ls ./*.tgz)
PACKED_PKG=$(tar -xzOf "$PACK_TGZ" package/package.json)
echo "$PACKED_PKG"
# workspace:^ -> ^2.4.1 (dependencies), workspace:~ -> ~2.4.1 (peerDependencies),
# workspace:* -> 2.4.1 (devDependencies), catalog: -> ^4.17.21
echo "$PACKED_PKG" | grep -q '"@pack-protocols/bar": "\^2.4.1"' \
  || { echo -e "${RED}FAIL: workspace:^ not rewritten to ^2.4.1${NC}"; cd ../../../../..; exit 1; }
echo "$PACKED_PKG" | grep -q '"@pack-protocols/bar": "~2.4.1"' \
  || { echo -e "${RED}FAIL: workspace:~ not rewritten to ~2.4.1${NC}"; cd ../../../../..; exit 1; }
echo "$PACKED_PKG" | grep -q '"@pack-protocols/bar": "2.4.1"' \
  || { echo -e "${RED}FAIL: workspace:* not rewritten to 2.4.1${NC}"; cd ../../../../..; exit 1; }
echo "$PACKED_PKG" | grep -q '"lodash": "\^4.17.21"' \
  || { echo -e "${RED}FAIL: catalog: not rewritten to ^4.17.21${NC}"; cd ../../../../..; exit 1; }
if echo "$PACKED_PKG" | grep -qE 'workspace:|catalog:'; then
    echo -e "${RED}FAIL: raw workspace:/catalog: protocol left in packed manifest${NC}"; cd ../../../../..; exit 1
fi
# The on-disk source manifest must be left untouched (still uses the protocols).
grep -q 'workspace:' package.json \
  || { echo -e "${RED}FAIL: source package.json was mutated by pack${NC}"; cd ../../../../..; exit 1; }
rm -f "$PACK_TGZ"
cd ../../../../..
echo -e "${GREEN}PASS: pm-pack workspace:/catalog: rewrite successful${NC}"

# Case 11: npm alias (npm: prefix) install
echo -e "${YELLOW}Case 11: npm alias install${NC}"
cd e2e/pm/npm-alias
rm -rf node_modules package-lock.json
utoo install --ignore-scripts || { echo -e "${RED}FAIL: utoo install failed for npm-alias${NC}"; exit 1; }

# 11a: simple alias — "my-jquery": "npm:jquery@3"
# node_modules/my-jquery should contain jquery's package.json
if [ ! -d "node_modules/my-jquery" ]; then
    echo -e "${RED}FAIL: my-jquery directory not created${NC}"
    exit 1
fi
ACTUAL_NAME=$(node -e "console.log(require('./node_modules/my-jquery/package.json').name)")
if [ "$ACTUAL_NAME" != "jquery" ]; then
    echo -e "${RED}FAIL: my-jquery should be jquery, got $ACTUAL_NAME${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: simple alias my-jquery -> jquery${NC}"

# 11b: scoped alias — "my-types": "npm:@types/node@^20"
if [ ! -d "node_modules/my-types" ]; then
    echo -e "${RED}FAIL: my-types directory not created${NC}"
    exit 1
fi
ACTUAL_NAME=$(node -e "console.log(require('./node_modules/my-types/package.json').name)")
if [ "$ACTUAL_NAME" != "@types/node" ]; then
    echo -e "${RED}FAIL: my-types should be @types/node, got $ACTUAL_NAME${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: scoped alias my-types -> @types/node${NC}"

# 11c: alias with transitive deps — "string-width-cjs": "npm:string-width@^4.2.0"
if [ ! -d "node_modules/string-width-cjs" ]; then
    echo -e "${RED}FAIL: string-width-cjs directory not created${NC}"
    exit 1
fi
ACTUAL_NAME=$(node -e "console.log(require('./node_modules/string-width-cjs/package.json').name)")
if [ "$ACTUAL_NAME" != "string-width" ]; then
    echo -e "${RED}FAIL: string-width-cjs should be string-width, got $ACTUAL_NAME${NC}"
    exit 1
fi
# string-width has transitive deps (strip-ansi, emoji-regex, is-fullwidth-code-point)
# they should be installed at top level, not nested under the alias
if [ ! -d "node_modules/strip-ansi" ]; then
    echo -e "${RED}FAIL: strip-ansi (transitive dep of string-width) not hoisted${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: alias with transitive deps string-width-cjs -> string-width${NC}"

# 11d: alias version mismatch — transitive dep nests when version doesn't match
# "undici-types": "npm:lodash@^4" occupies the directory name,
# but @types/node needs undici-types@~6.21.0. lodash 4.x doesn't satisfy ~6.21.0,
# so the real undici-types must be nested under my-types/node_modules/.
ACTUAL_NAME=$(node -e "console.log(require('./node_modules/undici-types/package.json').name)")
if [ "$ACTUAL_NAME" != "lodash" ]; then
    echo -e "${RED}FAIL: top-level undici-types should be lodash, got $ACTUAL_NAME${NC}"
    exit 1
fi
if [ ! -d "node_modules/my-types/node_modules/undici-types" ]; then
    echo -e "${RED}FAIL: real undici-types not nested under my-types (version mismatch should force nesting)${NC}"
    exit 1
fi
NESTED_NAME=$(node -e "console.log(require('./node_modules/my-types/node_modules/undici-types/package.json').name)")
if [ "$NESTED_NAME" != "undici-types" ]; then
    echo -e "${RED}FAIL: nested undici-types should be the real package, got $NESTED_NAME${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: alias version mismatch — real undici-types nested under my-types${NC}"

# 11e: alias version match — transitive dep reuses alias when version satisfies
# "ms": "npm:raw-body@2.1.3" occupies the ms directory.
# debug@4 depends on ms@^2.1.3. raw-body's version 2.1.3 satisfies ^2.1.3,
# so debug reuses the top-level ms (which is actually raw-body).
# This matches npm behavior: alias is pure directory-name occupation,
# resolution is by semver only, not by real package name.
ACTUAL_NAME=$(node -e "console.log(require('./node_modules/ms/package.json').name)")
if [ "$ACTUAL_NAME" != "raw-body" ]; then
    echo -e "${RED}FAIL: top-level ms should be raw-body, got $ACTUAL_NAME${NC}"
    exit 1
fi
if [ -d "node_modules/debug/node_modules/ms" ]; then
    echo -e "${RED}FAIL: debug should NOT have nested ms (version 2.1.3 satisfies ^2.1.3)${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: alias version match — debug reuses aliased ms (npm-compatible)${NC}"

# 11f: scoped alias name — alias name itself has a scope
# "@myorg/utils": "npm:lodash@^4" and "@myorg/types": "npm:@types/node@^20"
ACTUAL_NAME=$(node -e "console.log(require('./node_modules/@myorg/utils/package.json').name)")
if [ "$ACTUAL_NAME" != "lodash" ]; then
    echo -e "${RED}FAIL: @myorg/utils should be lodash, got $ACTUAL_NAME${NC}"
    exit 1
fi
ACTUAL_NAME=$(node -e "console.log(require('./node_modules/@myorg/types/package.json').name)")
if [ "$ACTUAL_NAME" != "@types/node" ]; then
    echo -e "${RED}FAIL: @myorg/types should be @types/node, got $ACTUAL_NAME${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: scoped alias names @myorg/utils -> lodash, @myorg/types -> @types/node${NC}"

echo -e "${GREEN}PASS: npm alias install successful${NC}"
cd ../../..

# Case: Verify optional dependencies with platform-specific binaries (rolldown)
echo -e "${YELLOW}Case: Verify optional dependencies (rolldown binding)${NC}"
OPTDEPS_DIR=$(mktemp -d)
pushd "$OPTDEPS_DIR"

cat > package.json << 'PKGJSON'
{
  "name": "test-optional-deps",
  "version": "1.0.0",
  "dependencies": {
    "rolldown": "1.0.0-beta.57"
  }
}
PKGJSON

echo "Installing rolldown (has platform-specific optional binding)..."
utoo install --registry=https://registry.npmjs.org

# Verify the platform-specific binding was installed
OS_NAME=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH_NAME=$(uname -m)
if [ "$OS_NAME" = "darwin" ]; then
    if [ "$ARCH_NAME" = "arm64" ] || [ "$ARCH_NAME" = "aarch64" ]; then
        BINDING="@rolldown/binding-darwin-arm64"
    else
        BINDING="@rolldown/binding-darwin-x64"
    fi
elif [ "$OS_NAME" = "linux" ]; then
    BINDING="@rolldown/binding-linux-x64-gnu"
fi

if [ -z "$BINDING" ] || [ ! -d "node_modules/$BINDING" ]; then
    echo -e "${RED}FAIL: Optional dependency check failed. Binding: '$BINDING' for $OS_NAME $ARCH_NAME${NC}"
    exit 1
fi

node -e "require('rolldown'); console.log('rolldown loaded successfully')"
echo -e "${GREEN}PASS: optional dependencies with platform bindings work correctly${NC}"

popd
rm -rf "$OPTDEPS_DIR"

# Case: Verify npm pack + npm install -g works (simulates setup-utoo flow)
echo -e "${YELLOW}Case: npm pack and install -g utoo${NC}"
PACK_DIR=$(mktemp -d)
INSTALL_PREFIX=$(mktemp -d)
REPO_ROOT=$(cd "$(dirname "$0")/.."; pwd)

pushd "$PACK_DIR"

# Build a utoo npm package using the vendor templates
mkdir -p pkg/bin
# Use the actual built binary from the e2e environment
UTOO_BIN=$(which utoo)
cp "$UTOO_BIN" pkg/bin/utoo
chmod +x pkg/bin/utoo

# Create package.json
cat > pkg/package.json << 'PKGJSON'
{
  "name": "utoo",
  "version": "0.0.0-e2e-test",
  "bin": { "utoo": "bin/utoo", "ut": "bin/utoo" },
  "scripts": { "postinstall": "echo postinstall-ok" }
}
PKGJSON

# Pack it
cd pkg
npm pack 2>&1
TARBALL=$(ls utoo-*.tgz)
echo "Packed: $TARBALL"

# Install globally to a temp prefix
npm install -g "$TARBALL" --prefix="$INSTALL_PREFIX" 2>&1
echo "Installed to: $INSTALL_PREFIX"

# Verify the binary works
INSTALLED_UTOO="$INSTALL_PREFIX/bin/utoo"
if [ ! -f "$INSTALLED_UTOO" ]; then
    # Try lib path on some systems
    INSTALLED_UTOO="$INSTALL_PREFIX/lib/node_modules/utoo/bin/utoo"
fi

if [ ! -f "$INSTALLED_UTOO" ]; then
    echo -e "${RED}FAIL: utoo binary not found after npm install -g${NC}"
    ls -R "$INSTALL_PREFIX" 2>/dev/null | head -20
    exit 1
fi

# Verify it's not a placeholder
if head -1 "$INSTALLED_UTOO" | grep -q "placeholder"; then
    echo -e "${RED}FAIL: installed binary is still a placeholder${NC}"
    exit 1
fi

"$INSTALLED_UTOO" --version
echo -e "${GREEN}PASS: npm pack + install -g works correctly${NC}"

# Regression: a global install run THROUGH the npm-style bin symlink must resolve
# the prefix to $INSTALL_PREFIX, NOT to utoo's own package dir. current_exe()
# resolves the symlink to <prefix>/lib/node_modules/utoo/bin/utoo, so a naive
# parent-dir inference would drop bins into utoo's package bin / node_modules
# instead of <prefix>/bin and <prefix>/lib/node_modules.
echo -e "${YELLOW}  Subtest: global install resolves npm-style prefix${NC}"
SYMLINK_UTOO="$INSTALL_PREFIX/bin/utoo"
if [ ! -L "$SYMLINK_UTOO" ] && [ ! -f "$SYMLINK_UTOO" ]; then
    echo -e "${RED}FAIL: expected npm bin entry at $SYMLINK_UTOO${NC}"
    exit 1
fi

"$SYMLINK_UTOO" install -g cowsay --registry=https://registry.npmjs.org \
    || { echo -e "${RED}FAIL: global install cowsay via npm-installed utoo${NC}"; exit 1; }

# Bin must land in <prefix>/bin (on PATH); package in <prefix>/lib/node_modules.
[ -e "$INSTALL_PREFIX/bin/cowsay" ] \
    || { echo -e "${RED}FAIL: cowsay bin not in <prefix>/bin${NC}"; ls -R "$INSTALL_PREFIX" | head -40; exit 1; }
[ -d "$INSTALL_PREFIX/lib/node_modules/cowsay" ] \
    || { echo -e "${RED}FAIL: cowsay not in <prefix>/lib/node_modules${NC}"; exit 1; }
# Must NOT leak into utoo's own package dir (the pre-fix bug).
[ ! -e "$INSTALL_PREFIX/lib/node_modules/utoo/bin/cowsay" ] \
    || { echo -e "${RED}FAIL: cowsay bin leaked into utoo's package bin dir${NC}"; exit 1; }
[ ! -e "$INSTALL_PREFIX/lib/node_modules/utoo/lib/node_modules/cowsay" ] \
    || { echo -e "${RED}FAIL: cowsay pkg leaked into utoo's package node_modules${NC}"; exit 1; }
echo -e "${GREEN}  ✓ PASS: npm-style prefix inference correct${NC}"

# UTOO_PREFIX env var overrides inference.
echo -e "${YELLOW}  Subtest: UTOO_PREFIX env override${NC}"
ENV_PREFIX=$(mktemp -d)
UTOO_PREFIX="$ENV_PREFIX" "$SYMLINK_UTOO" install -g semver --registry=https://registry.npmjs.org \
    || { echo -e "${RED}FAIL: global install semver with UTOO_PREFIX${NC}"; exit 1; }
[ -e "$ENV_PREFIX/bin/semver" ] \
    || { echo -e "${RED}FAIL: semver bin not in UTOO_PREFIX/bin${NC}"; ls -R "$ENV_PREFIX" | head -20; exit 1; }
[ -d "$ENV_PREFIX/lib/node_modules/semver" ] \
    || { echo -e "${RED}FAIL: semver not in UTOO_PREFIX/lib/node_modules${NC}"; exit 1; }
echo -e "${GREEN}  ✓ PASS: UTOO_PREFIX override works${NC}"
rm -rf "$ENV_PREFIX"

popd
rm -rf "$PACK_DIR" "$INSTALL_PREFIX"

# Case: `utoo link` must put the package's bins in <prefix>/bin (on PATH), not in
# the linked package's own bin dir. Use an isolated --prefix so we don't touch
# the runner's real global bin.
echo -e "${YELLOW}Case: utoo link puts bins in <prefix>/bin${NC}"
# Resolve to the physical path: on macOS `mktemp -d` lives under the /var ->
# /private/var symlink, which would make the relative bin symlink (pointing at
# the dev project on a different root) dangling — a test artifact, not a real
# prefix (real prefixes like /usr/local aren't symlinked).
LINK_PREFIX=$(cd "$(mktemp -d)" && pwd -P)
pushd "$REPO_ROOT/e2e/pm/link-with-bin"
utoo link --prefix "$LINK_PREFIX" \
    || { echo -e "${RED}FAIL: utoo link --prefix failed${NC}"; exit 1; }
popd
[ -e "$LINK_PREFIX/bin/link-bin-test" ] \
    || { echo -e "${RED}FAIL: linked bin not in <prefix>/bin${NC}"; ls -R "$LINK_PREFIX" | head -40; exit 1; }
[ -e "$LINK_PREFIX/lib/node_modules/link-bin-test" ] \
    || { echo -e "${RED}FAIL: linked package not in <prefix>/lib/node_modules${NC}"; exit 1; }
echo -e "${GREEN}PASS: utoo link puts bins in <prefix>/bin${NC}"
rm -rf "$LINK_PREFIX"

# Case: Verify ant-design-x install + build
echo -e "${YELLOW}Case: ant-design-x install and build${NC}"
ANTDX_DIR=$(mktemp -d)
git clone --branch next --single-branch --depth 1 https://github.com/ant-design/x.git "$ANTDX_DIR"
pushd "$ANTDX_DIR"

utoo install --ignore-scripts --registry=https://registry.npmjs.org || { echo -e "${RED}FAIL: utoo install failed for ant-design-x${NC}"; exit 1; }

echo -e "${GREEN}PASS: ant-design-x install successful${NC}"

popd
rm -rf "$ANTDX_DIR"

# Case: workspace with devDependency cycle
echo -e "${YELLOW}Case: workspace devDep cycle topology${NC}"
cd e2e/pm/workspace-cycle
rm -rf node_modules app/node_modules lib-a/node_modules lib-b/node_modules workspace.json

# deps --workspace-only should succeed despite lib-a <-> lib-b dev cycle
ut deps --workspace-only || { echo -e "${RED}FAIL: ut deps failed on workspace with devDep cycle${NC}"; exit 1; }

if [ ! -f "workspace.json" ]; then
    echo -e "${RED}FAIL: workspace.json not created${NC}"
    exit 1
fi

# Verify topology has multiple layers (not all in one layer)
LAYER_COUNT=$(node -e "const t=require('./workspace.json').topology; console.log(t.length)")
if [ "$LAYER_COUNT" -lt 2 ]; then
    echo -e "${RED}FAIL: expected at least 2 topology layers, got $LAYER_COUNT${NC}"
    exit 1
fi

# lib-b should appear before lib-a (lib-a prod-depends on lib-b)
FIRST_LAYER=$(node -e "console.log(JSON.stringify(require('./workspace.json').topology[0]))")
if ! echo "$FIRST_LAYER" | grep -q '"lib-b"'; then
    echo -e "${RED}FAIL: lib-b should be in first layer, got $FIRST_LAYER${NC}"
    exit 1
fi

echo -e "${GREEN}PASS: workspace devDep cycle handled correctly${NC}"
cd ../../..

# Case: workspace `prepare`/`postinstall` hooks run in topological order
# Regression guard for #2833: ut install must run lifecycle install hooks
# on workspace source packages (npm 7+ semantics) so that a consumer
# workspace can import the producer workspace's `prepare`-built output.
echo -e "${YELLOW}Case: workspace prepare hooks (issue #2833)${NC}"
cd e2e/pm/workspace-prepare
rm -rf node_modules app/node_modules lib-a/node_modules lib-b/node_modules
rm -rf lib-a/lib lib-a/.markers lib-b/lib package-lock.json

utoo install --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo install failed for workspace-prepare${NC}"; exit 1; }

# lib-a's `prepare` must produce lib/index.js BEFORE lib-b's prepare runs
if [ ! -f "lib-a/lib/index.js" ]; then
    echo -e "${RED}FAIL: lib-a/lib/index.js missing — lib-a prepare did not run${NC}"
    exit 1
fi
# lib-b's `prepare` only succeeds if lib-a's artifact already exists; the
# fact that lib-b/lib/index.js was produced proves topological ordering held
if [ ! -f "lib-b/lib/index.js" ]; then
    echo -e "${RED}FAIL: lib-b/lib/index.js missing — topological order broken${NC}"
    exit 1
fi
# postinstall on a workspace package must also fire (not just prepare) —
# and EXACTLY once. Regression guard for #3097: lib-a carries a `bin`, which
# made the rebuild collector queue its scripts from both the workspace source
# node and the node_modules link node, on top of the topological workspace
# walk, running postinstall 3×. The counter must read exactly 1.
if [ ! -f "lib-a/.markers/postinstall" ]; then
    echo -e "${RED}FAIL: lib-a postinstall did not run${NC}"
    exit 1
fi
count=$(wc -l < lib-a/.markers/postinstall | tr -d ' ')
if [ "$count" != "1" ]; then
    echo -e "${RED}FAIL: lib-a postinstall ran ${count}× (expected 1) — #3097 regression${NC}"
    exit 1
fi
# the workspace `bin` must still be linked into node_modules/.bin
if [ ! -e "node_modules/.bin/lib-a-cli" ]; then
    echo -e "${RED}FAIL: workspace bin lib-a-cli not linked into node_modules/.bin${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: utoo install ran workspace prepare/postinstall once in topo order${NC}"

# ut rebuild must re-run the same hooks (per issue: rebuild was also broken)
rm -rf lib-a/lib lib-a/.markers lib-b/lib

utoo rebuild \
  || { echo -e "${RED}FAIL: utoo rebuild failed for workspace-prepare${NC}"; exit 1; }

if [ ! -f "lib-a/lib/index.js" ] || [ ! -f "lib-b/lib/index.js" ]; then
    echo -e "${RED}FAIL: utoo rebuild did not run workspace prepare hooks${NC}"
    exit 1
fi
if [ ! -f "lib-a/.markers/postinstall" ]; then
    echo -e "${RED}FAIL: utoo rebuild did not run workspace postinstall${NC}"
    exit 1
fi
count=$(wc -l < lib-a/.markers/postinstall | tr -d ' ')
if [ "$count" != "1" ]; then
    echo -e "${RED}FAIL: utoo rebuild ran lib-a postinstall ${count}× (expected 1) — #3097 regression${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: utoo rebuild re-ran workspace prepare/postinstall once${NC}"

# --ignore-scripts must skip workspace hooks too (no surprise side effects)
rm -rf node_modules lib-a/lib lib-a/.markers lib-b/lib package-lock.json

utoo install --ignore-scripts --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo install --ignore-scripts failed${NC}"; exit 1; }

if [ -f "lib-a/lib/index.js" ] || [ -f "lib-a/.markers/postinstall" ]; then
    echo -e "${RED}FAIL: --ignore-scripts must skip workspace hooks${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: --ignore-scripts skips workspace hooks${NC}"

# Cleanup so re-runs start clean
rm -rf node_modules app/node_modules lib-a/node_modules lib-b/node_modules
rm -rf lib-a/lib lib-a/.markers lib-b/lib package-lock.json
cd ../../..

# Case: anonymous workspace packages (no `name` field) — npm/arborist
# fixtures like workspaces-need-update ship unnamed workspaces; ut install
# must derive a name from the folder layout (npm `@npmcli/name-from-folder`)
# and still run their lifecycle install hooks, otherwise an early bail like
# "Failed to get package name from package.json" aborts the whole walk.
echo -e "${YELLOW}Case: anonymous workspace packages (no name field)${NC}"
cd e2e/pm/workspace-anonymous
rm -rf node_modules anon-a/node_modules anon-b/node_modules
rm -f anon-a/marker-postinstall anon-b/marker-prepare package-lock.json

utoo install --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo install crashed on anonymous workspaces${NC}"; exit 1; }

# Both anonymous workspaces must have run their lifecycle hooks.
if [ ! -f "anon-a/marker-postinstall" ]; then
    echo -e "${RED}FAIL: anonymous workspace anon-a postinstall did not run${NC}"
    exit 1
fi
if [ ! -f "anon-b/marker-prepare" ]; then
    echo -e "${RED}FAIL: anonymous workspace anon-b prepare did not run${NC}"
    exit 1
fi
echo -e "${GREEN}PASS: anonymous workspace hooks ran via name-from-folder fallback${NC}"

rm -rf node_modules anon-a/node_modules anon-b/node_modules
rm -f anon-a/marker-postinstall anon-b/marker-prepare package-lock.json
cd ../../..

# Case: multi-workspace `ut run` log presentation
echo -e "${YELLOW}Case: ut run multi-workspace log presentation${NC}"
cd e2e/pm/run-workspaces

# Topology: lib-b (leaf) <- lib-a <- app
# Expected layer order: [lib-b] -> [lib-a] -> [app]

# Sub-case: --workspaces (all) should hit every workspace in topo order.
# NO_COLOR strips ANSI so we can grep the output reliably.
NO_COLOR=1 ut run build --workspaces > run-all.out 2>&1 \
  || { echo -e "${RED}FAIL: ut run build --workspaces exited non-zero${NC}"; cat run-all.out; exit 1; }
echo "--- ut run build --workspaces ---"
cat run-all.out
echo "---"

# Header line must name the script and the workspace count.
grep -q "Running build in 3 workspaces, 3 layers" run-all.out \
  || { echo -e "${RED}FAIL: missing multi-workspace header${NC}"; cat run-all.out; exit 1; }

# Each layer must be announced, one workspace per line of the header block.
grep -q "1:.*lib-b" run-all.out \
  || { echo -e "${RED}FAIL: header layer 1 should list lib-b${NC}"; cat run-all.out; exit 1; }
grep -q "2:.*lib-a" run-all.out \
  || { echo -e "${RED}FAIL: header layer 2 should list lib-a${NC}"; cat run-all.out; exit 1; }
grep -q "3:.*app" run-all.out \
  || { echo -e "${RED}FAIL: header layer 3 should list app${NC}"; cat run-all.out; exit 1; }

# Each spawned script announcement must carry a [workspace] prefix so
# concurrent output is distinguishable.
grep -q "\[lib-b\] echo building lib-b" run-all.out \
  || { echo -e "${RED}FAIL: missing [lib-b] prefixed script announcement${NC}"; cat run-all.out; exit 1; }
grep -q "\[lib-a\] echo building lib-a" run-all.out \
  || { echo -e "${RED}FAIL: missing [lib-a] prefixed script announcement${NC}"; cat run-all.out; exit 1; }
grep -q "\[app\] echo building app" run-all.out \
  || { echo -e "${RED}FAIL: missing [app] prefixed script announcement${NC}"; cat run-all.out; exit 1; }

# Topological ordering: lib-b's build must complete before app's starts.
# Use the actual echoed output lines (not the announcement) as the ordering
# witness — they're printed by the child process after ScriptService spawns.
LIB_B_LINE=$(grep -n "^building lib-b$" run-all.out | head -1 | cut -d: -f1)
APP_LINE=$(grep -n "^building app$" run-all.out | head -1 | cut -d: -f1)
if [ -z "$LIB_B_LINE" ] || [ -z "$APP_LINE" ] || [ "$LIB_B_LINE" -ge "$APP_LINE" ]; then
    echo -e "${RED}FAIL: topological order broken (lib-b line=$LIB_B_LINE, app line=$APP_LINE)${NC}"
    cat run-all.out
    exit 1
fi

# Sub-case: explicit multi --workspace should respect topology for the subset.
NO_COLOR=1 ut run build --workspace lib-b --workspace app > run-subset.out 2>&1 \
  || { echo -e "${RED}FAIL: ut run build --workspace lib-b --workspace app exited non-zero${NC}"; cat run-subset.out; exit 1; }
echo "--- ut run build --workspace lib-b --workspace app ---"
cat run-subset.out
echo "---"

# Header must reflect subset size (2 workspaces), not the full topology.
grep -q "Running build in 2 workspaces" run-subset.out \
  || { echo -e "${RED}FAIL: subset header should count 2 workspaces${NC}"; cat run-subset.out; exit 1; }

# lib-a was NOT selected, so its announcement must not appear.
if grep -q "\[lib-a\]" run-subset.out; then
    echo -e "${RED}FAIL: lib-a should be excluded from subset run${NC}"
    cat run-subset.out
    exit 1
fi

# Subset must still preserve topology: lib-b before app.
LIB_B_LINE=$(grep -n "^building lib-b$" run-subset.out | head -1 | cut -d: -f1)
APP_LINE=$(grep -n "^building app$" run-subset.out | head -1 | cut -d: -f1)
if [ -z "$LIB_B_LINE" ] || [ -z "$APP_LINE" ] || [ "$LIB_B_LINE" -ge "$APP_LINE" ]; then
    echo -e "${RED}FAIL: subset topological order broken${NC}"
    cat run-subset.out
    exit 1
fi

# Sub-case: glob filter should expand to matching workspaces only.
NO_COLOR=1 ut run build --workspace 'lib-*' > run-glob.out 2>&1 \
  || { echo -e "${RED}FAIL: ut run build --workspace 'lib-*' exited non-zero${NC}"; cat run-glob.out; exit 1; }
echo "--- ut run build --workspace 'lib-*' ---"
cat run-glob.out
echo "---"

grep -q "Running build in 2 workspaces" run-glob.out \
  || { echo -e "${RED}FAIL: glob should match exactly 2 workspaces${NC}"; cat run-glob.out; exit 1; }
grep -q "\[lib-a\]" run-glob.out \
  || { echo -e "${RED}FAIL: glob should include lib-a${NC}"; cat run-glob.out; exit 1; }
grep -q "\[lib-b\]" run-glob.out \
  || { echo -e "${RED}FAIL: glob should include lib-b${NC}"; cat run-glob.out; exit 1; }
if grep -q "\[app\]" run-glob.out; then
    echo -e "${RED}FAIL: glob 'lib-*' should not match app${NC}"
    cat run-glob.out
    exit 1
fi

# Sub-case: --if-present should skip workspaces missing the script silently,
# without blank `✓` rows or empty `▶ N/M` layer separators.
NO_COLOR=1 ut run test --workspaces --if-present > run-ifpresent.out 2>&1 \
  || { echo -e "${RED}FAIL: ut run test --workspaces --if-present exited non-zero${NC}"; cat run-ifpresent.out; exit 1; }
echo "--- ut run test --workspaces --if-present ---"
cat run-ifpresent.out
echo "---"

# Only lib-b has a `test` script — expect exactly one result line.
grep -q "\[lib-b\] echo testing lib-b" run-ifpresent.out \
  || { echo -e "${RED}FAIL: missing [lib-b] test announcement${NC}"; cat run-ifpresent.out; exit 1; }
if grep -q "\[lib-a\]\|\[app\]" run-ifpresent.out; then
    echo -e "${RED}FAIL: --if-present should not announce workspaces without the script${NC}"
    cat run-ifpresent.out
    exit 1
fi

# No blank `✓` rows (the old bug: one empty tick per skipped workspace).
if grep -Eq '^✓[[:space:]]*$' run-ifpresent.out; then
    echo -e "${RED}FAIL: --if-present printed blank ✓ rows${NC}"
    cat run-ifpresent.out
    exit 1
fi

# No layer separator for layers where every workspace is skipped.
# lib-b sits in layer 1; layers 2 (lib-a) and 3 (app) must not appear.
if grep -q "▶ 2/3\|▶ 3/3" run-ifpresent.out; then
    echo -e "${RED}FAIL: --if-present printed empty layer separator${NC}"
    cat run-ifpresent.out
    exit 1
fi

rm -f run-all.out run-subset.out run-glob.out run-ifpresent.out
echo -e "${GREEN}PASS: ut run multi-workspace log presentation${NC}"
cd ../../..

# Case: pnpm migration (eggjs/egg)
echo -e "${YELLOW}Case: pnpm migration (eggjs/egg)${NC}"
EGG_DIR=$(mktemp -d)
git clone --branch next --single-branch --depth 1 https://github.com/eggjs/egg.git "$EGG_DIR"
pushd "$EGG_DIR"

utoo install --from pnpm --ignore-scripts --registry=https://registry.npmjs.org || { echo -e "${RED}FAIL: utoo install --from pnpm failed for eggjs/egg${NC}"; exit 1; }

# Verify workspaces field was added to package.json
node -e "
  const pkg = require('./package.json');
  const ws = pkg.workspaces;
  if (!ws || !Array.isArray(ws)) throw new Error('workspaces not set');
  if (!ws.includes('packages/*')) throw new Error('missing packages/*');
  console.log('  workspaces:', ws.length, 'patterns');
"

# Verify overrides were added
node -e "
  const pkg = require('./package.json');
  if (!pkg.overrides) throw new Error('overrides not set');
  if (!pkg.overrides.vite) throw new Error('vite override missing');
  console.log('  overrides:', Object.keys(pkg.overrides).length, 'entries');
"

# Verify .utoo.toml was created with catalogs
[ -f ".utoo.toml" ] || { echo -e "${RED}FAIL: .utoo.toml not created${NC}"; exit 1; }
grep -q 'lodash' .utoo.toml || { echo -e "${RED}FAIL: catalog missing lodash${NC}"; exit 1; }
grep -q 'path-to-regexp' .utoo.toml || { echo -e "${RED}FAIL: named catalog missing${NC}"; exit 1; }

# Verify node_modules was created (install ran successfully)
[ -d "node_modules" ] || { echo -e "${RED}FAIL: node_modules not created${NC}"; exit 1; }

echo -e "${GREEN}PASS: pnpm migration (eggjs/egg)${NC}"

popd
rm -rf "$EGG_DIR"

# Case: install-node + esbuild postinstall
echo -e "${YELLOW}Case: install-node + esbuild${NC}"
ESBUILD_DIR=$(mktemp -d)
cat > "$ESBUILD_DIR/package.json" << 'EOF'
{
  "name": "install-node-esbuild-test",
  "dependencies": {
    "esbuild": "0.27.0"
  },
  "engines": {
    "install-node": "20"
  }
}
EOF
pushd "$ESBUILD_DIR"

utoo install --registry=https://registry.npmjs.org || { echo -e "${RED}FAIL: utoo install failed for install-node + esbuild${NC}"; exit 1; }

# Verify local node is available
node_modules/.bin/node -v || { echo -e "${RED}FAIL: local node not executable${NC}"; exit 1; }

# Verify esbuild postinstall ran and binary works
node_modules/.bin/esbuild --version || { echo -e "${RED}FAIL: esbuild not executable${NC}"; exit 1; }

# Regression guard: running install a second time on an `engines.install-node`
# project must not re-resolve the lockfile. The outdated check used to see the
# synthetic `node-bin-*` optionalDependencies on the lock side only and judge
# the lock stale on every call → infinite re-resolve. `save_package_lock` uses
# write-tmp + rename, so a rewrite always changes the inode; an identical
# inode across invocations is the strongest signal that the cached lock was
# reused.
INODE_BEFORE=$(node -e "console.log(require('fs').statSync('package-lock.json').ino)")
utoo install --registry=https://registry.npmjs.org 2>&1 | tee warm.out \
  || { echo -e "${RED}FAIL: warm utoo install failed for install-node + esbuild${NC}"; exit 1; }
INODE_AFTER=$(node -e "console.log(require('fs').statSync('package-lock.json').ino)")
if [ "$INODE_BEFORE" != "$INODE_AFTER" ]; then
    echo -e "${RED}FAIL: package-lock.json was regenerated on warm install (install-node optionalDeps asymmetry)${NC}"
    exit 1
fi
if grep -q "package-lock.json is outdated" warm.out; then
    echo -e "${RED}FAIL: outdated warning on warm install — outdated check still asymmetric${NC}"
    cat warm.out
    exit 1
fi
rm -f warm.out
echo -e "${GREEN}PASS: install-node + esbuild (cold + warm, lockfile reused)${NC}"

popd
rm -rf "$ESBUILD_DIR"

# Case: broken pipe should not panic
echo -e "${YELLOW}Case: broken pipe handling${NC}"
SIGPIPE_DIR=$(mktemp -d)
pushd "$SIGPIPE_DIR"
cat > package.json <<'PKGJSON'
{
  "name": "sigpipe-test",
  "version": "1.0.0",
  "scripts": {
    "prepare": "echo line1 && echo line2 && echo line3"
  }
}
PKGJSON
# Pipe script output to head — closes the pipe early. utoo should not panic.
EXIT_CODE=0
ut run prepare 2>/dev/null | head -1 > /dev/null || EXIT_CODE=$?
if [ $EXIT_CODE -ne 0 ] && [ $EXIT_CODE -ne 141 ]; then
    echo -e "${RED}FAIL: broken pipe caused exit code $EXIT_CODE (expected 0 or 141)${NC}"
    popd; rm -rf "$SIGPIPE_DIR"; exit 1
fi
echo -e "${GREEN}PASS: broken pipe handled cleanly${NC}"

# Verify that non-zero exit codes from scripts are propagated correctly
cat > package.json <<'PKGJSON'
{
  "name": "sigpipe-test",
  "version": "1.0.0",
  "scripts": {
    "fail": "exit 141"
  }
}
PKGJSON
EXIT_CODE=0
ut run fail 2>/dev/null || EXIT_CODE=$?
if [ $EXIT_CODE -eq 0 ]; then
    echo -e "${RED}FAIL: script exiting 141 should propagate non-zero exit code${NC}"
    popd; rm -rf "$SIGPIPE_DIR"; exit 1
fi
echo -e "${GREEN}PASS: script exit code propagated correctly (got $EXIT_CODE)${NC}"

popd
rm -rf "$SIGPIPE_DIR"

# Case: legacyPeerDeps defaults to true (peer deps not auto-installed)
echo -e "${YELLOW}Case: legacyPeerDeps default (skip peer deps)${NC}"
cd e2e/pm/peer-deps
rm -rf node_modules package-lock.json

utoo install --ignore-scripts --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo install failed for peer-deps test${NC}"; exit 1; }

# react-dom has react as a peerDependency.
# With legacyPeerDeps=true (default), react should NOT be auto-installed.
if [ -d "node_modules/react" ]; then
    echo -e "${RED}FAIL: react was auto-installed as peer dep — legacyPeerDeps default is broken${NC}"
    exit 1
fi

# react-dom itself must be installed
if [ ! -d "node_modules/react-dom" ]; then
    echo -e "${RED}FAIL: react-dom not installed${NC}"
    exit 1
fi

echo -e "${GREEN}PASS: legacyPeerDeps default — peer deps not auto-installed${NC}"

# Sub-case: explicitly disable legacyPeerDeps via config — peer deps SHOULD be installed
echo -e "${YELLOW}Case: legacyPeerDeps=false (peer deps auto-installed)${NC}"
rm -rf node_modules package-lock.json

# Write local config to disable legacyPeerDeps
cat > .utoo.toml <<'EOF'
[values]
legacy-peer-deps = "false"
EOF

utoo install --ignore-scripts --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo install failed for peer-deps test (legacy=false)${NC}"; rm -f .utoo.toml; exit 1; }
rm -f .utoo.toml

# With legacyPeerDeps=false, react SHOULD be auto-installed as a peer dep of react-dom
if [ ! -d "node_modules/react" ]; then
    echo -e "${RED}FAIL: react was NOT auto-installed — legacyPeerDeps=false is broken${NC}"
    exit 1
fi

echo -e "${GREEN}PASS: legacyPeerDeps=false — peer deps auto-installed${NC}"
cd ../../..

# Case: tarball permission normalization
# google-protobuf@4.0.2 ships files at 0o640 in its tarball (package.json,
# google-protobuf.js, README.md, LICENSE*). Preserving raw tar modes leaves
# them unreadable by "other" and breaks container/cross-user reads — npm and
# pnpm both normalize to 0o644. This is a regression guard for that behavior.
echo -e "${YELLOW}Case: tarball permission normalization (google-protobuf)${NC}"
PERM_DIR=$(mktemp -d)
pushd "$PERM_DIR"
cat > package.json <<'PKGJSON'
{
  "name": "perm-normalize-test",
  "version": "1.0.0",
  "dependencies": {
    "google-protobuf": "4.0.2"
  }
}
PKGJSON

# Force a cold extract so the normalization path actually runs
rm -rf ~/.cache/nm/google-protobuf

utoo install --ignore-scripts --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo install failed for perm-normalize-test${NC}"; popd; rm -rf "$PERM_DIR"; exit 1; }

# Every file under the package must be world-readable.
# Without the fix, the 0o640 files above are listed here and the case fails.
NON_READABLE=$(find node_modules/google-protobuf -type f ! -perm -0004 -print)
if [ -n "$NON_READABLE" ]; then
    echo -e "${RED}FAIL: files missing other-read bit after install:${NC}"
    echo "$NON_READABLE"
    ls -la node_modules/google-protobuf/
    popd; rm -rf "$PERM_DIR"; exit 1
fi

# Spot-check one of the known-0o640 files landed at exactly 0o644 (not 0o640,
# not 0o755 — we should not grant exec unless the tar entry had it).
# `stat -c` (GNU) / `stat -f` (BSD) differ; use Node for portability.
MODE=$(node -e "console.log((require('fs').statSync('node_modules/google-protobuf/package.json').mode & 0o777).toString(8))")
if [ "$MODE" != "644" ]; then
    echo -e "${RED}FAIL: package.json mode is 0o$MODE, expected 0o644${NC}"
    popd; rm -rf "$PERM_DIR"; exit 1
fi
echo -e "${GREEN}PASS: tarball permissions normalized to 0o644${NC}"

popd
rm -rf "$PERM_DIR"

# ═══════════════════════════════════════════════════════════════
# Case: Test 'utoo add' alias works the same as 'utoo install'
# ═══════════════════════════════════════════════════════════════
echo -e "${YELLOW}Case: Test 'utoo add' alias (Issue #2608)${NC}"
ADD_TEST_DIR=$(mktemp -d)
pushd "$ADD_TEST_DIR"

cat > package.json << 'PKGJSON'
{
  "name": "test-add-alias",
  "version": "1.0.0",
  "dependencies": {}
}
PKGJSON

# Test 1: Basic add command
echo -e "${YELLOW}  Subtest 1.1: utoo add react${NC}"
utoo add react --ignore-scripts --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo add react${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
[ -d "node_modules/react" ] || { echo -e "${RED}FAIL: react not installed${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: utoo add react${NC}"

# Test 2: Add with -D flag (dev dependency)
echo -e "${YELLOW}  Subtest 1.2: utoo add lodash -D${NC}"
utoo add lodash -D --ignore-scripts \
  || { echo -e "${RED}FAIL: utoo add -D${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
grep -q '"devDependencies"' package.json || { echo -e "${RED}FAIL: -D flag not working${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
grep -q '"lodash"' package.json || { echo -e "${RED}FAIL: lodash not in devDependencies${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: utoo add lodash -D${NC}"

# Test 3: Short alias ut add
echo -e "${YELLOW}  Subtest 1.3: ut add express${NC}"
ut add express --ignore-scripts \
  || { echo -e "${RED}FAIL: ut add express${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
[ -d "node_modules/express" ] || { echo -e "${RED}FAIL: express not installed${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: ut add express${NC}"

# Test 4: Add with -O flag (optional dependency)
echo -e "${YELLOW}  Subtest 1.4: utoo add debug -O${NC}"
utoo add debug@4.3.4 -O --ignore-scripts \
  || { echo -e "${RED}FAIL: utoo add -O${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
grep -q '"optionalDependencies"' package.json || { echo -e "${RED}FAIL: -O flag not working${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: utoo add debug -O${NC}"

# Test 5: Add with --save-peer flag (peer dependency)
echo -e "${YELLOW}  Subtest 1.5: utoo add typescript --save-peer${NC}"
utoo add typescript@5.0.4 --save-peer --ignore-scripts \
  || { echo -e "${RED}FAIL: utoo add --save-peer${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
grep -q '"peerDependencies"' package.json || { echo -e "${RED}FAIL: --save-peer flag not working${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: utoo add typescript --save-peer${NC}"

# Test 6: Help text verification
echo -e "${YELLOW}  Subtest 1.6: Help text shows add alias${NC}"
utoo --help | grep -i "add" > /dev/null || { echo -e "${RED}FAIL: 'add' not in help${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
utoo add --help > /dev/null || { echo -e "${RED}FAIL: utoo add --help failed${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: Help text includes add alias${NC}"

# Test 7: Backward compatibility - install still works
echo -e "${YELLOW}  Subtest 1.7: Backward compatibility - utoo install${NC}"
rm -rf node_modules package-lock.json
utoo install react --ignore-scripts --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo install still required${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
[ -d "node_modules/react" ] || { echo -e "${RED}FAIL: react not installed via install${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: utoo install still works${NC}"

# Test 8: Backward compatibility - 'i' alias still works
echo -e "${YELLOW}  Subtest 1.8: Backward compatibility - ut i${NC}"
rm -rf node_modules package-lock.json
ut i lodash --ignore-scripts \
  || { echo -e "${RED}FAIL: ut i still required${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
[ -d "node_modules/lodash" ] || { echo -e "${RED}FAIL: lodash not installed via 'i'${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: ut i still works${NC}"

# Test 9: Add multiple packages at once
echo -e "${YELLOW}  Subtest 1.9: Add multiple packages${NC}"
rm -rf node_modules package-lock.json
utoo add is-array is-object --ignore-scripts --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo add multiple packages${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
[ -d "node_modules/is-array" ] || { echo -e "${RED}FAIL: is-array not installed${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
[ -d "node_modules/is-object" ] || { echo -e "${RED}FAIL: is-object not installed${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: Add multiple packages${NC}"

# Test 10: Add with version spec
echo -e "${YELLOW}  Subtest 1.10: Add with version spec${NC}"
rm -rf node_modules package-lock.json
utoo add 'semver@^7.0.0' --ignore-scripts --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo add with version spec${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
[ -d "node_modules/semver" ] || { echo -e "${RED}FAIL: semver not installed${NC}"; popd; rm -rf "$ADD_TEST_DIR"; exit 1; }
echo -e "${GREEN}  ✓ PASS: Add with version spec${NC}"

popd
rm -rf "$ADD_TEST_DIR"
echo -e "${GREEN}PASS: All 'utoo add' alias tests successful${NC}"

# ═══════════════════════════════════════════════════════════════
# Case: Test 'utoo add' global install
# ═══════════════════════════════════════════════════════════════
echo -e "${YELLOW}Case: Test 'utoo add' global install (Issue #2608)${NC}"
echo -e "${YELLOW}  Subtest 2.1: utoo add -g cowsay${NC}"
utoo add -g cowsay --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo add -g cowsay${NC}"; exit 1; }
which cowsay >/dev/null 2>&1 || { echo -e "${RED}FAIL: cowsay not in PATH after global add${NC}"; exit 1; }
echo -e "${GREEN}  ✓ PASS: utoo add -g works${NC}"

# ═══════════════════════════════════════════════════════════════
# Case: prod-reachable devDependency must not be marked `dev`
# ═══════════════════════════════════════════════════════════════
# `p-timeout` is declared as a root devDependency (shallow dev path) but is
# also a prod dependency of `sdk-base` (a root prod dependency). Since it is
# reachable through a prod chain it must be a prod node — no `"dev": true` —
# matching npm. Regression test for the demand resolver leaving stale dev marks
# on prod-reachable transitive deps.
echo -e "${YELLOW}Case: prod-reachable devDependency is not marked dev${NC}"
cd e2e/pm/dev-prod-dedup
rm -rf node_modules package-lock.json
rm -rf ~/.cache/nm
utoo install --ignore-scripts --registry=https://registry.npmjs.org \
  || { echo -e "${RED}FAIL: utoo install failed for dev-prod-dedup${NC}"; exit 1; }
node -e '
const lock = require("./package-lock.json");
const sdkBase = lock.packages["node_modules/sdk-base"];
const pTimeout = lock.packages["node_modules/p-timeout"];
if (!sdkBase) { console.error("sdk-base missing from lockfile"); process.exit(1); }
if (!pTimeout) { console.error("p-timeout missing from lockfile"); process.exit(1); }
if (sdkBase.dev === true) { console.error("sdk-base wrongly marked dev"); process.exit(1); }
if (pTimeout.dev === true) {
  console.error("REGRESSION: p-timeout is prod-reachable via sdk-base but marked dev:true");
  process.exit(1);
}
' || { echo -e "${RED}FAIL: prod-reachable dep marked dev in lockfile${NC}"; exit 1; }
echo -e "${GREEN}PASS: prod-reachable devDependency correctly not marked dev${NC}"
cd ../../../

echo -e "${GREEN}All e2e tests passed successfully!${NC}"
