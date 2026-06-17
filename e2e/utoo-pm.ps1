#!/usr/bin/env pwsh
$ErrorActionPreference = "Stop"

# Colors for output
function Write-Red { Write-Host $args -ForegroundColor Red }
function Write-Green { Write-Host $args -ForegroundColor Green }
function Write-Yellow { Write-Host $args -ForegroundColor Yellow }

Write-Yellow "Starting utoo-pm windows e2e tests..."
Write-Host "utoo path: $(if (Get-Command utoo -ErrorAction SilentlyContinue) { (Get-Command utoo).Source } else { 'not found' })"
Write-Host "ut path: $(if (Get-Command ut -ErrorAction SilentlyContinue) { (Get-Command ut).Source } else { 'not found' })"
Write-Host "node arch: $(node -e 'console.log(process.arch)')"

ut config set registry https://registry.npmjs.org --global

# Case 1: Clone and install ant-design-x (next)
Write-Yellow "Case 1: Clone and install ant-design-x (next)"
try {
    Push-Location e2e/pm/ant-design-x
    
    if (-not (Test-Path "ant-design-x")) {
        git clone --branch next --single-branch https://github.com/ant-design/x.git ant-design-x
    }
    
    Push-Location ant-design-x
    try {
        Write-Host "Installing dependencies for ant-design-x (next)..."
        utoo deps
        if ($LASTEXITCODE -ne 0) { throw "utoo deps failed for ant-design-x (next)" }

        utoo install --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw "utoo install failed for ant-design-x (next)" }

        utoo rebuild
        if ($LASTEXITCODE -ne 0) { throw "utoo rebuild failed for ant-design-x (next)" }

        Write-Green "PASS: ant-design-x (next) cloned and installed"
    }
    finally {
        Pop-Location
    }
}
finally {
    Pop-Location
}

# Case 2: Clone and install ant-design
Write-Yellow "Case 2: Clone and install ant-design"
try {
    Push-Location e2e/pm/ant-design
    
    if (-not (Test-Path "ant-design")) {
        git clone --depth=1 --single-branch https://github.com/ant-design/ant-design.git
    }
    
    Push-Location ant-design
    try {
        Write-Host "Installing dependencies for ant-design..."
        # Use --ignore-scripts to skip prepare hook that causes @swc/core native binding issues on Windows
        utoo install --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw "utoo install failed for ant-design" }
        
        Write-Green "PASS: ant-design cloned and installed"
    }
    finally {
        Pop-Location
    }
}
finally {
    Pop-Location
}

# Case 3: antd-test project install
Write-Yellow "Case 3: antd-test project install"
try {
    Push-Location e2e/pm/antd-test
    
    utoo install
    if ($LASTEXITCODE -ne 0) { throw "utoo install failed for antd-test" }
    
    if (-not (Test-Path "node_modules")) {
        throw "node_modules directory not created"
    }
    
    if (-not (Test-Path "node_modules/antd")) {
        throw "antd package not installed"
    }
    
    Write-Green "PASS: antd-test install successful"
}
finally {
    Pop-Location
}

# Case 4: local-package link test
Write-Yellow "Case 4: local-package link test"
try {
    Push-Location e2e/pm/local-package
    
    utoo install
    if ($LASTEXITCODE -ne 0) { throw "utoo install failed for local-package" }
    
    utoo link
    if ($LASTEXITCODE -ne 0) { throw "utoo link failed for local-package" }

    Write-Green "PASS: local-package link successful"
}
finally {
    Pop-Location
}

# Case: `utoo link` must put the package's bin shim in the prefix bin dir (the
# prefix ROOT on Windows), not in the linked package's own bin dir. Use an
# isolated --prefix so we don't touch the runner's real global bin.
Write-Yellow "Case: utoo link puts bins in prefix bin dir"
$linkPrefix = Join-Path $env:TEMP "utoo-e2e-linkprefix-$(Get-Random)"
try {
    Push-Location e2e/pm/link-with-bin
    utoo link --prefix $linkPrefix
    if ($LASTEXITCODE -ne 0) { throw "utoo link --prefix failed" }

    $linkShim = @(
        (Join-Path $linkPrefix "link-bin-test.cmd"),
        (Join-Path $linkPrefix "link-bin-test")
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $linkShim) {
        Get-ChildItem -Recurse $linkPrefix | Select-Object FullName | Format-Table
        throw "linked bin shim not found in <prefix> root"
    }
    if (-not (Test-Path (Join-Path $linkPrefix "node_modules\link-bin-test"))) {
        throw "linked package not in <prefix>\node_modules"
    }
    Write-Green "PASS: utoo link puts bins in prefix bin dir"
}
finally {
    Pop-Location
    Remove-Item -Recurse -Force $linkPrefix -ErrorAction SilentlyContinue
}

# Case 5: antd-test secondary install
Write-Yellow "Case 5: antd-test secondary install"
try {
    Push-Location e2e/pm/antd-test
    
    utoo install
    if ($LASTEXITCODE -ne 0) { throw "utoo install failed for antd-test (secondary)" }
    
    if (-not (Test-Path "node_modules/lodash")) {
        throw "lodash package not installed in secondary update"
    }
    
    Write-Green "PASS: antd-test secondary install successful"
}
finally {
    Pop-Location
}

# Case 6: antd-test deps tree
Write-Yellow "Case 6: antd-test deps tree"
try {
    Push-Location e2e/pm/antd-test
    
    utoo deps
    if ($LASTEXITCODE -ne 0) { throw "utoo deps failed for antd-test" }
    
    if (-not (Test-Path "package-lock.json")) {
        throw "utoo deps did not generate output"
    }
    
    $lockContent = Get-Content package-lock.json -Raw
    if ($lockContent -notmatch "antd") {
        throw "utoo deps output does not contain antd"
    }
    
    if ($lockContent -notmatch "react") {
        throw "utoo deps output does not contain react"
    }
    
    Write-Green "PASS: antd-test deps tree successful"
}
finally {
    Pop-Location
}

# Case 7: test global install
Write-Yellow "Case 7: cowsay global install/uninstall"
utoo install -g cowsay
if ($LASTEXITCODE -ne 0) {
    Write-Red "FAIL: global install cowsay failed"
    exit 1
}

if (-not (Get-Command cowsay -ErrorAction SilentlyContinue)) {
    Write-Red "FAIL: cowsay not found in PATH after global install"
    exit 1
}

Write-Green "PASS: cowsay global install successful"

# Case 8: reinstall ant-design
Write-Yellow "Case 8: Clone and install ant-design by npmjs.org"
try {
    Push-Location e2e/pm/ant-design
    
    git clean -dfx
    
    Write-Host "Installing dependencies for ant-design by npmjs.org..."
    utoo install --registry=https://registry.npmjs.org --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "utoo install failed for ant-design (npmjs.org)" }

    Write-Green "PASS: ant-design cloned and installed"
}
finally {
    Pop-Location
}

# Case: Verify optional dependencies with platform-specific binaries (rolldown)
Write-Yellow "Case: Verify optional dependencies (rolldown binding)"
$optDepsDir = Join-Path $env:TEMP "utoo-e2e-optdeps-$(Get-Random)"
try {
    New-Item -ItemType Directory -Path $optDepsDir -Force | Out-Null
    Push-Location $optDepsDir

    # Create a minimal project that depends on rolldown
    @{
        name = "test-optional-deps"
        version = "1.0.0"
        dependencies = @{
            rolldown = "1.0.0-beta.57"
        }
    } | ConvertTo-Json | Set-Content "package.json"

    Write-Host "Installing rolldown (has win32-x64 optional binding)..."
    utoo install --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "utoo install failed for rolldown test" }

    # Verify the Windows-specific binding was installed
    $nodeArch = (node -p "process.arch").Trim()
    $bindingPath = "node_modules/@rolldown/binding-win32-$($nodeArch)-msvc"
    if (-not (Test-Path $bindingPath)) {
        throw "Optional dependency @rolldown/binding-win32-$($nodeArch)-msvc was NOT installed"
    }

    # Verify the binding actually works
    node -e "require('rolldown'); console.log('rolldown loaded successfully')"
    if ($LASTEXITCODE -ne 0) { throw "rolldown failed to load (binding may be broken)" }

    Write-Green "PASS: optional dependencies with platform bindings work correctly"
}
finally {
    Pop-Location
    Remove-Item -Recurse -Force $optDepsDir -ErrorAction SilentlyContinue
}

# Case: Verify npm pack + npm install -g works (simulates setup-utoo flow)
Write-Yellow "Case: npm pack and install -g utoo"
$packDir = Join-Path $env:TEMP "utoo-e2e-pack-$(Get-Random)"
$installPrefix = Join-Path $env:TEMP "utoo-e2e-prefix-$(Get-Random)"
try {
    New-Item -ItemType Directory -Path "$packDir\pkg\bin" -Force | Out-Null
    New-Item -ItemType Directory -Path $installPrefix -Force | Out-Null

    # Copy the actual built binary
    $utooBin = (Get-Command utoo).Source
    Copy-Item $utooBin "$packDir\pkg\bin\utoo.exe"

    # Create package.json
    @{
        name = "utoo"
        version = "0.0.0-e2e-test"
        bin = @{ utoo = "bin/utoo"; ut = "bin/utoo" }
        scripts = @{ postinstall = "echo postinstall-ok" }
    } | ConvertTo-Json | Set-Content "$packDir\pkg\package.json"

    # Pack
    Push-Location "$packDir\pkg"
    npm pack 2>&1
    $tarball = Get-ChildItem "utoo-*.tgz" | Select-Object -First 1
    Write-Host "Packed: $($tarball.Name)"

    # Install globally to temp prefix
    npm install -g $tarball.FullName "--prefix=$installPrefix" 2>&1
    Write-Host "Installed to: $installPrefix"

    # Verify the binary exists in the package dir (not the npm shim)
    $candidatePaths = @(
        (Join-Path $installPrefix "node_modules\utoo\bin\utoo.exe"),
        (Join-Path $installPrefix "node_modules\utoo\bin\utoo")
    )
    $installedUtoo = $null
    foreach ($p in $candidatePaths) {
        if (Test-Path $p) { $installedUtoo = $p; break }
    }
    if (-not $installedUtoo) {
        Write-Host "Contents of install prefix:"
        Get-ChildItem -Recurse $installPrefix | Select-Object FullName | Format-Table
        throw "utoo binary not found after npm install -g"
    }

    Write-Host "Found binary at: $installedUtoo"

    # Verify it's not a placeholder (read first bytes, not text)
    $bytes = [System.IO.File]::ReadAllBytes($installedUtoo)
    $header = [System.Text.Encoding]::ASCII.GetString($bytes, 0, [Math]::Min(100, $bytes.Length))
    if ($header.Contains("placeholder")) {
        throw "installed binary is still a placeholder"
    }

    & $installedUtoo --version
    if ($LASTEXITCODE -ne 0) { throw "utoo --version failed" }

    Write-Green "PASS: npm pack + install -g works correctly"

    # Regression: a global install run THROUGH the npm-installed binary must
    # resolve the prefix to $installPrefix. On Windows npm places executable
    # shims directly in the prefix ROOT (not a bin subdir) and packages under
    # <prefix>\node_modules; a naive parent-dir inference would drop them under
    # utoo's own package dir instead.
    Write-Yellow "  Subtest: global install resolves npm-style prefix (Windows)"
    & $installedUtoo install -g cowsay --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "global install cowsay via npm-installed utoo failed" }

    $cowsayShim = @(
        (Join-Path $installPrefix "cowsay.cmd"),
        (Join-Path $installPrefix "cowsay")
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $cowsayShim) {
        Write-Host "Contents of install prefix:"
        Get-ChildItem -Recurse $installPrefix | Select-Object FullName | Format-Table
        throw "cowsay shim not found in <prefix> root"
    }
    if (-not (Test-Path (Join-Path $installPrefix "node_modules\cowsay"))) {
        throw "cowsay package not in <prefix>\node_modules"
    }
    # Must NOT leak into utoo's own package dir (the pre-fix bug).
    if (Test-Path (Join-Path $installPrefix "node_modules\utoo\node_modules\cowsay")) {
        throw "cowsay leaked into utoo's package node_modules"
    }
    Write-Green "  PASS: npm-style prefix inference correct (Windows)"

    # UTOO_PREFIX env var overrides inference.
    Write-Yellow "  Subtest: UTOO_PREFIX env override (Windows)"
    $envPrefix = Join-Path $env:TEMP "utoo-e2e-envprefix-$(Get-Random)"
    try {
        $env:UTOO_PREFIX = $envPrefix
        & $installedUtoo install -g semver --registry=https://registry.npmjs.org
        if ($LASTEXITCODE -ne 0) { throw "global install semver with UTOO_PREFIX failed" }
        $semverShim = @(
            (Join-Path $envPrefix "semver.cmd"),
            (Join-Path $envPrefix "semver")
        ) | Where-Object { Test-Path $_ } | Select-Object -First 1
        if (-not $semverShim) { throw "semver shim not found in UTOO_PREFIX root" }
        if (-not (Test-Path (Join-Path $envPrefix "node_modules\semver"))) {
            throw "semver not in UTOO_PREFIX\node_modules"
        }
        Write-Green "  PASS: UTOO_PREFIX override works (Windows)"
    }
    finally {
        Remove-Item Env:\UTOO_PREFIX -ErrorAction SilentlyContinue
        Remove-Item -Recurse -Force $envPrefix -ErrorAction SilentlyContinue
    }
}
finally {
    Pop-Location
    Remove-Item -Recurse -Force $packDir -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $installPrefix -ErrorAction SilentlyContinue
}

# Case: Verify ant-design-x install + build on Windows
Write-Yellow "Case: ant-design-x install and build"
$antdxDir = Join-Path $env:TEMP "utoo-e2e-antdx-$(Get-Random)"
try {
    git clone --branch next --single-branch --depth 1 https://github.com/ant-design/x.git $antdxDir
    Push-Location $antdxDir

    utoo install --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "utoo install failed for ant-design-x" }

    Write-Green "PASS: ant-design-x install successful"
}
finally {
    Pop-Location
    Remove-Item -Recurse -Force $antdxDir -ErrorAction SilentlyContinue
}

# Case: pnpm migration (eggjs/egg)
Write-Yellow "Case: pnpm migration (eggjs/egg)"
$eggDir = Join-Path $env:TEMP "utoo-e2e-egg-$(Get-Random)"
try {
    git clone --branch next --single-branch --depth 1 https://github.com/eggjs/egg.git $eggDir
    Push-Location $eggDir

    utoo install --from pnpm --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "utoo install --from pnpm failed for eggjs/egg" }

    # Verify workspaces field was added to package.json
    node -e "const pkg = require('./package.json'); const ws = pkg.workspaces; if (!ws || !Array.isArray(ws)) throw new Error('workspaces not set'); if (!ws.includes('packages/*')) throw new Error('missing packages/*'); console.log('  workspaces:', ws.length, 'patterns');"
    if ($LASTEXITCODE -ne 0) { throw "workspaces field verification failed" }

    # Verify overrides were added
    node -e "const pkg = require('./package.json'); if (!pkg.overrides) throw new Error('overrides not set'); if (!pkg.overrides.vite) throw new Error('vite override missing'); console.log('  overrides:', Object.keys(pkg.overrides).length, 'entries');"
    if ($LASTEXITCODE -ne 0) { throw "overrides field verification failed" }

    # Verify .utoo.toml was created with catalogs
    if (-not (Test-Path ".utoo.toml")) { throw ".utoo.toml not created" }
    $tomlContent = Get-Content .utoo.toml -Raw
    if ($tomlContent -notmatch 'lodash') { throw "catalog missing lodash" }
    if ($tomlContent -notmatch 'path-to-regexp') { throw "named catalog missing" }

    # Verify node_modules was created (install ran successfully)
    if (-not (Test-Path "node_modules")) { throw "node_modules not created" }

    Write-Green "PASS: pnpm migration (eggjs/egg)"
}
finally {
    Pop-Location
    Remove-Item -Recurse -Force $eggDir -ErrorAction SilentlyContinue
}

# Case: install-node + esbuild postinstall
Write-Yellow "Case: install-node + esbuild"
$esbuildDir = Join-Path $env:TEMP "utoo-e2e-esbuild-$(Get-Random)"
try {
    New-Item -ItemType Directory -Path $esbuildDir -Force | Out-Null
    Push-Location $esbuildDir

    @'
{
  "name": "install-node-esbuild-test",
  "dependencies": {
    "esbuild": "0.27.0"
  },
  "engines": {
    "install-node": "20"
  }
}
'@ | Set-Content -Path "package.json"

    utoo install --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "utoo install failed for install-node + esbuild" }

    # Verify local node is available
    & "node_modules/.bin/node" -v
    if ($LASTEXITCODE -ne 0) { throw "local node not executable" }

    # Verify esbuild postinstall ran and binary works
    & "node_modules/.bin/esbuild" --version
    if ($LASTEXITCODE -ne 0) { throw "esbuild not executable" }

    Write-Green "PASS: install-node + esbuild"
}
finally {
    Pop-Location
    Remove-Item -Recurse -Force $esbuildDir -ErrorAction SilentlyContinue
}

# ═══════════════════════════════════════════════════════════════
# Case: Test 'utoo add' alias works the same as 'utoo install'
# ═══════════════════════════════════════════════════════════════
Write-Yellow "Case: Test 'utoo add' alias (Issue #2608)"
$addTestDir = Join-Path $env:TEMP "utoo-e2e-add-$(Get-Random)"
try {
    New-Item -ItemType Directory -Path $addTestDir -Force | Out-Null
    Push-Location $addTestDir

    @{
        name = "test-add-alias"
        version = "1.0.0"
        dependencies = @{}
    } | ConvertTo-Json | Set-Content "package.json"

    # Test 1: Basic add command
    Write-Yellow "  Subtest 1.1: utoo add react"
    utoo add react --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "utoo add react failed" }
    if (-not (Test-Path "node_modules/react")) { throw "react not installed" }
    Write-Green "  ✓ PASS: utoo add react"

    # Test 2: Add with -D flag (dev dependency)
    Write-Yellow "  Subtest 1.2: utoo add lodash -D"
    utoo add lodash -D --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "utoo add -D failed" }
    $pkgContent = Get-Content package.json -Raw
    if ($pkgContent -notmatch '"devDependencies"') { throw "-D flag not working" }
    Write-Green "  ✓ PASS: utoo add lodash -D"

    # Test 3: Short alias ut add
    Write-Yellow "  Subtest 1.3: ut add express"
    ut add express --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "ut add express failed" }
    if (-not (Test-Path "node_modules/express")) { throw "express not installed" }
    Write-Green "  ✓ PASS: ut add express"

    # Test 4: Add with -O flag (optional dependency)
    Write-Yellow "  Subtest 1.4: utoo add debug -O"
    utoo add debug@4.3.4 -O --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "utoo add -O failed" }
    $pkgContent = Get-Content package.json -Raw
    if ($pkgContent -notmatch '"optionalDependencies"') { throw "-O flag not working" }
    Write-Green "  ✓ PASS: utoo add debug -O"

    # Test 5: Add with --save-peer flag (peer dependency)
    Write-Yellow "  Subtest 1.5: utoo add typescript --save-peer"
    utoo add typescript@5.0.4 --save-peer --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "utoo add --save-peer failed" }
    $pkgContent = Get-Content package.json -Raw
    if ($pkgContent -notmatch '"peerDependencies"') { throw "--save-peer flag not working" }
    Write-Green "  ✓ PASS: utoo add typescript --save-peer"

    # Test 6: Help text verification
    Write-Yellow "  Subtest 1.6: Help text shows add alias"
    $helpOutput = utoo --help 2>&1 | Out-String
    if ($helpOutput -notmatch "add") { throw "'add' not in help" }
    utoo add --help | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "utoo add --help failed" }
    Write-Green "  ✓ PASS: Help text includes add alias"

    # Test 7: Backward compatibility - install still works
    Write-Yellow "  Subtest 1.7: Backward compatibility - utoo install"
    Remove-Item -Recurse -Force node_modules, package-lock.json -ErrorAction SilentlyContinue
    utoo install react --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "utoo install still required" }
    if (-not (Test-Path "node_modules/react")) { throw "react not installed via install" }
    Write-Green "  ✓ PASS: utoo install still works"

    # Test 8: Backward compatibility - 'i' alias still works
    Write-Yellow "  Subtest 1.8: Backward compatibility - ut i"
    Remove-Item -Recurse -Force node_modules, package-lock.json -ErrorAction SilentlyContinue
    ut i lodash --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "ut i still required" }
    if (-not (Test-Path "node_modules/lodash")) { throw "lodash not installed via 'i'" }
    Write-Green "  ✓ PASS: ut i still works"

    # Test 9: Add multiple packages at once
    Write-Yellow "  Subtest 1.9: Add multiple packages"
    Remove-Item -Recurse -Force node_modules, package-lock.json -ErrorAction SilentlyContinue
    utoo add is-array is-object --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "utoo add multiple packages failed" }
    if (-not (Test-Path "node_modules/is-array")) { throw "is-array not installed" }
    if (-not (Test-Path "node_modules/is-object")) { throw "is-object not installed" }
    Write-Green "  ✓ PASS: Add multiple packages"

    # Test 10: Add with version spec
    Write-Yellow "  Subtest 1.10: Add with version spec"
    Remove-Item -Recurse -Force node_modules, package-lock.json -ErrorAction SilentlyContinue
    utoo add 'semver@^7.0.0' --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "utoo add with version spec failed" }
    if (-not (Test-Path "node_modules/semver")) { throw "semver not installed" }
    Write-Green "  ✓ PASS: Add with version spec"

    Write-Green "PASS: All 'utoo add' alias tests successful"
}
finally {
    Pop-Location
    Remove-Item -Recurse -Force $addTestDir -ErrorAction SilentlyContinue
}

# ═══════════════════════════════════════════════════════════════
# Case: Test 'utoo add' global install
# ═══════════════════════════════════════════════════════════════
Write-Yellow "Case: Test 'utoo add' global install (Issue #2608)"
Write-Yellow "  Subtest 2.1: utoo add -g cowsay"
utoo add -g cowsay --registry=https://registry.npmjs.org
if ($LASTEXITCODE -ne 0) { throw "utoo add -g cowsay failed" }
if (-not (Get-Command cowsay -ErrorAction SilentlyContinue)) {
    throw "cowsay not in PATH after global add"
}
Write-Green "  ✓ PASS: utoo add -g works"

# ═══════════════════════════════════════════════════════════════
# Case: file: tarball located OUTSIDE the project root (Windows)
# ═══════════════════════════════════════════════════════════════
# Windows mirror of the bash Case 8.5b. A tarball outside the project root
# yields a root-relative lockfile entry carrying `..` (e.g. "file:../foo.tgz").
# BFS hashes the cache slot from the canonical absolute path, while install
# re-absolutizes the `..`-laden lockfile path; without lexical `..`-collapse
# (which must also handle the Windows `C:` drive prefix) the two disagree and
# the clone fails with "file tarball cache not found". No other e2e exercises
# this on win32-msvc.
Write-Yellow "Case: file: tarball outside project root (Windows)"
$extDir = Join-Path $env:TEMP "utoo-e2e-exttgz-$(Get-Random)"
try {
    New-Item -ItemType Directory -Path "$extDir\src\ext-tarball-pkg" -Force | Out-Null
    New-Item -ItemType Directory -Path "$extDir\app" -Force | Out-Null

    @{ name = "ext-tarball-pkg"; version = "5.6.7"; main = "index.js" } |
        ConvertTo-Json | Set-Content "$extDir\src\ext-tarball-pkg\package.json"
    Set-Content "$extDir\src\ext-tarball-pkg\index.js" "module.exports = 567;"

    # Pack the leaf package into a tarball that sits OUTSIDE the app dir, so the
    # lockfile's root-relative `file:` path carries a `..`.
    Push-Location "$extDir\src\ext-tarball-pkg"
    try {
        npm pack 2>&1 | Out-Null
        $tgz = Get-ChildItem "ext-tarball-pkg-*.tgz" | Select-Object -First 1
        Move-Item $tgz.FullName "$extDir\ext-tarball-pkg.tgz"
    }
    finally { Pop-Location }

    $tgzSpec = "file:" + ((Join-Path $extDir "ext-tarball-pkg.tgz") -replace '\\', '/')
    @{
        name         = "ext-tarball-app"
        version      = "1.0.0"
        private      = $true
        dependencies = @{ "ext-tarball-pkg" = $tgzSpec }
    } | ConvertTo-Json | Set-Content "$extDir\app\package.json"

    Push-Location "$extDir\app"
    try {
        utoo install --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw "install failed for tarball outside project root" }

        if (-not (Test-Path "node_modules\ext-tarball-pkg\package.json")) {
            throw "ext-tarball-pkg not materialized into node_modules"
        }
        $ver = node -p "require('./node_modules/ext-tarball-pkg/package.json').version"
        if ($ver.Trim() -ne "5.6.7") { throw "ext-tarball-pkg expected v5.6.7, got $ver" }

        # Lockfile must stay portable: a root-relative `file:` path, not absolute.
        node -e "const lock=require('./package-lock.json');const tar=lock.packages['node_modules/ext-tarball-pkg']||{};const r=tar.resolved||'';if(!r.startsWith('file:')){console.error('resolved not file:',r);process.exit(1);}const p=r.slice(5);if(/^[A-Za-z]:/.test(p)||p[0]==='/'){console.error('resolved should be root-relative, got absolute:',r);process.exit(1);}console.log('  lockfile resolved (portable):',r);"
        if ($LASTEXITCODE -ne 0) { throw "lockfile entry wrong for outside-root tarball" }

        Write-Green "PASS: file: tarball outside project root installs (Windows)"
    }
    finally { Pop-Location }
}
finally {
    Remove-Item -Recurse -Force $extDir -ErrorAction SilentlyContinue
}

# ═══════════════════════════════════════════════════════════════
# Case: latest binary-mirror-config still parses under our schema
# ═══════════════════════════════════════════════════════════════
# `binary-mirror-config`'s `mirrors.china` map is parsed into a strongly-typed
# schema (crates/pm/src/service/binary.rs). The whole map deserializes as one
# unit, so a single drifted entry fails the parse and silently disables the
# China mirror layer for every package — `get_envs()`/`update_package_binary()`
# swallow the error as a debug log. (`flow-bin` ships `replaceHost` as a bare
# string, not an array, which previously broke the parse.)
#
# Guard against upstream drift: install against a non-npm registry (npm.org has
# no mirror layer and is skipped) with --verbose, then assert the config loaded
# and did NOT fail to parse. flow-bin is the dep on purpose — it is the exact
# entry that regressed.
Write-Yellow "Case: latest binary-mirror-config parses under our schema"
$bmcDir = Join-Path $env:TEMP "utoo-e2e-bmc-$(Get-Random)"
try {
    New-Item -ItemType Directory -Path $bmcDir -Force | Out-Null
    @{
        name         = "binary-mirror-config-parse-test"
        version      = "1.0.0"
        dependencies = @{ "flow-bin" = "0.180.0" }
    } | ConvertTo-Json | Set-Content "$bmcDir\package.json"

    Push-Location $bmcDir
    try {
        # --ignore-scripts keeps this fast (no native binary download); the
        # mirror config is loaded on the clone path regardless of script exec.
        $bmcOut = utoo install --registry=https://registry.npmmirror.com --ignore-scripts --verbose 2>&1
        $bmcOut | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "utoo install failed for binary-mirror-config parse test" }

        if ($bmcOut -match "Failed to parse binary mirror config") {
            throw "latest binary-mirror-config no longer matches our schema (mirrors.china drift)"
        }
        if (-not ($bmcOut -match "Binary mirror config loaded:")) {
            throw "binary-mirror-config was never parsed (registry unreachable or mirror layer skipped)"
        }

        Write-Green "PASS: latest binary-mirror-config parses cleanly"
    }
    finally { Pop-Location }
}
finally {
    Remove-Item -Recurse -Force $bmcDir -ErrorAction SilentlyContinue
}

Write-Green "All e2e tests passed successfully!"