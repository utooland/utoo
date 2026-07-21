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

        # Reuse round-trip guard: add then remove a dependency; the lockfile must
        # return byte-identical, proving the reuse path preserves the existing
        # ~5900-entry workspace tree (and its symlinks) instead of redrawing it.
        Write-Yellow "Case 1b: ant-design-x add/remove dependency keeps the tree stable"
        Copy-Item package-lock.json package-lock.baseline.json -Force
        $baseLinks = node -e "const l=require('./package-lock.json').packages;console.log(Object.keys(l).filter(k=>l[k].link).length)"
        utoo install cowsay@1.5.0 --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw "add cowsay failed (ant-design-x)" }
        node -e "const b=require('./package-lock.baseline.json').packages,n=require('./package-lock.json').packages;const kb=new Set(Object.keys(b));const added=Object.keys(n).filter(k=>!kb.has(k));const removed=[...kb].filter(k=>!n[k]);if(!n['node_modules/cowsay']){console.error('cowsay not added');process.exit(1)}if(removed.length){console.error('REGRESSION: adding cowsay dropped '+removed.length+' existing entries (tree explosion)');process.exit(1)}if(added.length>80){console.error('REGRESSION: adding cowsay grew the tree by '+added.length+' entries');process.exit(1)}console.log('add: +'+added.length+' entries (cowsay subtree), 0 existing dropped')"
        if ($LASTEXITCODE -ne 0) { throw "adding cowsay exploded the tree (ant-design-x)" }
        utoo uninstall cowsay --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw "remove cowsay failed (ant-design-x)" }
        $nowLinks = node -e "const l=require('./package-lock.json').packages;console.log(Object.keys(l).filter(k=>l[k].link).length)"
        if ($baseLinks -ne $nowLinks) { throw "workspace symlinks changed ($baseLinks -> $nowLinks) after add/remove (ant-design-x)" }
        if ((Get-FileHash package-lock.baseline.json).Hash -ne (Get-FileHash package-lock.json).Hash) {
            throw "add+remove did not round-trip the lockfile (reuse churned the tree, ant-design-x)"
        }
        Remove-Item package-lock.baseline.json -Force
        Write-Green "PASS: ant-design-x add/remove keeps the tree stable (byte-identical round-trip)"
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

        # Same reuse round-trip guard on the large non-workspace tree (~5100
        # entries): add a leaf dependency, then remove it, and the lockfile must
        # return to a byte-identical baseline.
        Write-Yellow "Case 2b: ant-design add/remove dependency keeps the tree stable"
        Copy-Item package-lock.json package-lock.baseline.json -Force
        utoo install cowsay@1.5.0 --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw "add cowsay failed (ant-design)" }
        node -e "const b=require('./package-lock.baseline.json').packages,n=require('./package-lock.json').packages;const kb=new Set(Object.keys(b));const added=Object.keys(n).filter(k=>!kb.has(k));const removed=[...kb].filter(k=>!n[k]);if(!n['node_modules/cowsay']){console.error('cowsay not added');process.exit(1)}if(removed.length){console.error('REGRESSION: adding cowsay dropped '+removed.length+' existing entries (tree explosion)');process.exit(1)}if(added.length>80){console.error('REGRESSION: adding cowsay grew the tree by '+added.length+' entries');process.exit(1)}console.log('add: +'+added.length+' entries (cowsay subtree), 0 existing dropped')"
        if ($LASTEXITCODE -ne 0) { throw "adding cowsay exploded the tree (ant-design)" }
        utoo uninstall cowsay --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw "remove cowsay failed (ant-design)" }
        if ((Get-FileHash package-lock.baseline.json).Hash -ne (Get-FileHash package-lock.json).Hash) {
            throw "add+remove did not round-trip the lockfile (reuse churned the tree, ant-design)"
        }
        Remove-Item package-lock.baseline.json -Force
        Write-Green "PASS: ant-design add/remove keeps the tree stable (byte-identical round-trip)"
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
    New-Item -ItemType Directory -Path "$packDir\pkg\bin", "$packDir\platform\bin" -Force | Out-Null
    New-Item -ItemType Directory -Path $installPrefix -Force | Out-Null

    # Pack the current Windows native binary as the optional platform package.
    $utooBin = (Get-Command utoo).Source
    Copy-Item $utooBin "$packDir\platform\bin\utoo.exe"
    $nodeArch = (node -p "process.arch").Trim()
    $platformName = "utoo-win32-$nodeArch"
    @{
        name = "@utoo/$platformName"
        version = "0.0.0-e2e-test"
        os = @("win32")
        cpu = @($nodeArch)
    } | ConvertTo-Json | Set-Content "$packDir\platform\package.json"
    Push-Location "$packDir\platform"
    npm pack 2>&1
    $platformTarball = (Get-ChildItem "utoo-*.tgz" | Select-Object -First 1).FullName
    Pop-Location

    # Build the main package from the real immutable launcher templates.
    $tplDir = Join-Path (Split-Path $PSScriptRoot -Parent) "vendor/templates"
    Copy-Item "$tplDir\launcher.utoo.js.template" "$packDir\pkg\bin\launcher.js"
    Copy-Item "$tplDir\utoo.utoo.js.template" "$packDir\pkg\bin\utoo.js"
    Copy-Item "$tplDir\utx.utoo.js.template" "$packDir\pkg\bin\utx.js"
    @{
        name = "utoo"
        version = "0.0.0-e2e-test"
        bin = @{ utoo = "bin/utoo.js"; ut = "bin/utoo.js"; utx = "bin/utx.js" }
        optionalDependencies = @{ "@utoo/$platformName" = "file:$platformTarball" }
    } | ConvertTo-Json | Set-Content "$packDir\pkg\package.json"

    # Pack
    Push-Location "$packDir\pkg"
    npm pack 2>&1
    $tarball = Get-ChildItem "utoo-*.tgz" | Select-Object -First 1
    Write-Host "Packed: $($tarball.Name)"

    # Install with lifecycle scripts disabled. The launcher path must still work.
    npm install -g $tarball.FullName "--prefix=$installPrefix" --ignore-scripts 2>&1
    Write-Host "Installed to: $installPrefix"

    # Verify npm generated the public shim and installed the optional artifact.
    $installedUtoo = Join-Path $installPrefix "utoo.cmd"
    if (-not (Test-Path $installedUtoo)) {
        Write-Host "Contents of install prefix:"
        Get-ChildItem -Recurse $installPrefix | Select-Object FullName | Format-Table
        throw "utoo.cmd launcher not found after npm install -g"
    }
    $nativeArtifact = Get-ChildItem -Path $installPrefix -Recurse -Filter "utoo.exe" |
        Where-Object { $_.FullName -match [regex]::Escape("@utoo") } |
        Select-Object -First 1
    if (-not $nativeArtifact) { throw "optional utoo.exe artifact was not installed" }
    $installedManifest = Get-Content (Join-Path $installPrefix "node_modules\utoo\package.json") -Raw |
        ConvertFrom-Json
    if ($null -ne $installedManifest.scripts) {
        throw "installed main package unexpectedly contains lifecycle scripts"
    }

    & $installedUtoo --version
    if ($LASTEXITCODE -ne 0) { throw "utoo --version failed" }

    Write-Green "PASS: npm pack + install -g works correctly"

    # The native executable is nested in the optional package. The launcher
    # must propagate the public managed-package root so global operations still
    # infer $installPrefix.
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

# Case: the real immutable launcher resolves and runs the optional Windows
# artifact without lifecycle hooks or prefix mutation. Stage exactly the files
# published by vendor/scripts/npm-utoo.sh and npm-binary.sh, plus inert npm shim
# fixtures that must remain untouched.
Write-Yellow "Case: immutable npm launcher runs the Windows platform artifact"
$tplDir = Join-Path (Split-Path $PSScriptRoot -Parent) "vendor/templates"
$bootPrefix = Join-Path $env:TEMP "utoo-e2e-launcher-$(Get-Random)"
try {
    $pkgDir = Join-Path $bootPrefix "node_modules\utoo"
    $optDir = Join-Path $bootPrefix "node_modules\@utoo\utoo-win32-x64"
    New-Item -ItemType Directory -Path "$pkgDir\bin", "$optDir\bin" -Force | Out-Null

    # Render the main package exactly like vendor/scripts/npm-utoo.sh.
    (Get-Content "$tplDir\utoo.package.json.template" -Raw).Replace("{{version}}", "9.9.9-e2e") |
        Set-Content "$pkgDir\package.json"
    Copy-Item "$tplDir\launcher.utoo.js.template" "$pkgDir\bin\launcher.js"
    Copy-Item "$tplDir\utoo.utoo.js.template" "$pkgDir\bin\utoo.js"
    Copy-Item "$tplDir\utx.utoo.js.template" "$pkgDir\bin\utx.js"

    # Optional platform dep ships the PE binary directly as bin/utoo.exe.
    Copy-Item (Get-Command utoo).Source "$optDir\bin\utoo.exe"
    '{"name":"@utoo/utoo-win32-x64","version":"9.9.9-e2e","os":["win32"],"cpu":["x64","arm64"]}' |
        Set-Content "$optDir\package.json"

    # Simulate package-manager shims. Runtime execution must not replace or
    # remove any of them.
    foreach ($s in "utoo", "utoo.cmd", "utoo.ps1", "ut", "ut.cmd", "ut.ps1", "utx.ps1") {
        Set-Content (Join-Path $bootPrefix $s) "shim-fixture-$s"
    }
    $launcherHash = (Get-FileHash "$pkgDir\bin\launcher.js").Hash
    $binaryHash = (Get-FileHash "$optDir\bin\utoo.exe").Hash

    node "$pkgDir\bin\utoo.js" --version
    if ($LASTEXITCODE -ne 0) { throw "Windows launcher failed to run utoo.exe" }

    foreach ($s in "utoo", "utoo.cmd", "utoo.ps1", "ut", "ut.cmd", "ut.ps1", "utx.ps1") {
        $content = (Get-Content (Join-Path $bootPrefix $s) -Raw).Trim()
        if ($content -ne "shim-fixture-$s") { throw "launcher modified package-manager shim: $s" }
    }
    if (Test-Path (Join-Path $bootPrefix "utoo.exe")) { throw "launcher copied utoo.exe into prefix" }
    if ((Get-FileHash "$pkgDir\bin\launcher.js").Hash -ne $launcherHash) { throw "launcher mutated itself" }
    if ((Get-FileHash "$optDir\bin\utoo.exe").Hash -ne $binaryHash) { throw "launcher mutated native artifact" }

    Write-Green "PASS: launcher executes utoo.exe without hooks or prefix mutation"
}
finally {
    Remove-Item -Recurse -Force $bootPrefix -ErrorAction SilentlyContinue
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
# Case: overrides target a local file: tarball (nested + direct)
# ═══════════════════════════════════════════════════════════════
# An override value may be any spec npm accepts — including a local `file:`
# tarball — not just a registry version. Override resolution used to send the
# target straight to the registry as a version, so `file:x.tgz` became a bogus
# version lookup → 404 and the whole install aborted. Here a transitive dep
# (is-number, pulled in by is-odd) is overridden to a locally packed tarball,
# both via a nested rule (is-odd > is-number) and a direct rule.
Write-Yellow "Case: overrides -> local file: tarball (nested + direct)"
$ovDir = Join-Path $env:TEMP "utoo-e2e-override-tgz-$(Get-Random)"
try {
    New-Item -ItemType Directory -Path "$ovDir\src\is-number" -Force | Out-Null
    @{ name = "is-number"; version = "9.9.9"; main = "index.js" } |
        ConvertTo-Json | Set-Content "$ovDir\src\is-number\package.json"
    Set-Content "$ovDir\src\is-number\index.js" "module.exports = () => 'OVERRIDE-LOCAL-TGZ';"

    Push-Location "$ovDir\src\is-number"
    try {
        npm pack 2>&1 | Out-Null
        $tgz = Get-ChildItem "is-number-*.tgz" | Select-Object -First 1
        Move-Item $tgz.FullName "$ovDir\is-number.tgz"
    }
    finally { Pop-Location }

    Push-Location $ovDir
    try {
        # Nested rule: is-odd > is-number -> file: tarball.
        @{
            name         = "ov-file-tarball-test"
            version      = "1.0.0"
            dependencies = @{ "is-odd" = "3.0.1" }
            overrides    = @{ "is-odd" = @{ "is-number" = "file:./is-number.tgz" } }
        } | ConvertTo-Json -Depth 5 | Set-Content "package.json"

        utoo install --registry=https://registry.npmjs.org --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw "install failed for nested file: tarball override" }
        $ver = (node -p "require('./node_modules/is-number/package.json').version").Trim()
        if ($ver -ne "9.9.9") { throw "nested override did not resolve to local tarball (is-number=$ver, want 9.9.9)" }
        $out = (node -p "require('./node_modules/is-number')()").Trim()
        if ($out -ne "OVERRIDE-LOCAL-TGZ") { throw "overridden is-number content wrong (got '$out')" }
        node -e "const l=require('./package-lock.json');const r=(l.packages['node_modules/is-number']||{}).resolved||'';if(!r.startsWith('file:')){console.error('is-number not pinned to file: tarball, got',r);process.exit(1);}"
        if ($LASTEXITCODE -ne 0) { throw "lockfile did not pin overridden is-number to file: tarball" }

        # Direct (non-nested) rule must work too.
        @{
            name         = "ov-file-tarball-test"
            version      = "1.0.0"
            dependencies = @{ "is-odd" = "3.0.1" }
            overrides    = @{ "is-number" = "file:./is-number.tgz" }
        } | ConvertTo-Json -Depth 5 | Set-Content "package.json"
        Remove-Item -Recurse -Force node_modules, package-lock.json -ErrorAction SilentlyContinue

        utoo install --registry=https://registry.npmjs.org --ignore-scripts
        if ($LASTEXITCODE -ne 0) { throw "install failed for direct file: tarball override" }
        $ver2 = (node -p "require('./node_modules/is-number/package.json').version").Trim()
        if ($ver2 -ne "9.9.9") { throw "direct override did not resolve to local tarball (is-number=$ver2, want 9.9.9)" }

        Write-Green "PASS: overrides -> local file: tarball (nested + direct)"
    }
    finally { Pop-Location }
}
finally {
    Remove-Item -Recurse -Force $ovDir -ErrorAction SilentlyContinue
}

# ═══════════════════════════════════════════════════════════════
# Case: overrides → file: DIRECTORY must fail with a clear error
# ═══════════════════════════════════════════════════════════════
# A `file:` directory dep installs as a symlink (Link node, no manifest), which
# the manifest-returning override path can't express. It must fail with an
# actionable "use a tarball" message rather than silently resolving it as a
# registry version (→ 404). Guards the boundary against a regression.
Write-Yellow "Case: overrides -> file: directory errors clearly"
$ovdDir = Join-Path $env:TEMP "utoo-e2e-override-dir-$(Get-Random)"
try {
    New-Item -ItemType Directory -Path "$ovdDir\local-is-number" -Force | Out-Null
    @{ name = "is-number"; version = "9.9.9"; main = "index.js" } |
        ConvertTo-Json | Set-Content "$ovdDir\local-is-number\package.json"
    Set-Content "$ovdDir\local-is-number\index.js" "module.exports = () => 'DIR';"

    Push-Location $ovdDir
    try {
        @{
            name         = "ov-file-dir-test"
            version      = "1.0.0"
            dependencies = @{ "is-odd" = "3.0.1" }
            overrides    = @{ "is-number" = "file:./local-is-number" }
        } | ConvertTo-Json -Depth 5 | Set-Content "package.json"

        # Join to a single string: `2>&1` yields an array of lines, and
        # `-match`/`-notmatch` over an array filter elements (truthy) rather
        # than test a boolean.
        $ovdOut = (utoo install --registry=https://registry.npmjs.org --ignore-scripts 2>&1 | Out-String)
        if ($LASTEXITCODE -eq 0) {
            Write-Host $ovdOut
            throw "file: directory override should not install successfully"
        }
        if ($ovdOut -notmatch "directory overrides") {
            Write-Host $ovdOut
            throw "file: directory override gave the wrong error (want 'directory overrides … use a tarball')"
        }
        if ($ovdOut -match "No matching version") {
            Write-Host $ovdOut
            throw "file: directory override regressed into a registry version lookup (404)"
        }
        Write-Green "PASS: overrides -> file: directory errors clearly"
    }
    finally { Pop-Location }
}
finally {
    Remove-Item -Recurse -Force $ovdDir -ErrorAction SilentlyContinue
}


# ═══════════════════════════════════════════════════════════════
# Case: bundled binary-mirror-config applies on the npmmirror registry
# ═══════════════════════════════════════════════════════════════
# `binary-mirror-config` is bundled at build time (crates/pm/src/service/
# binary-mirror-config.json) — no runtime fetch — and its parse under our strict
# schema is guarded by a unit test. This case is the integration check: install
# against npmmirror (one registry detected as semver-capable) with --verbose
# and assert the bundled mirror layer initialized during the install (the debug
# line fires when the config is first read on a non-skipped package). flow-bin is
# the dep on purpose — its bare-string `replaceHost` is the entry that once broke
# parse.
Write-Yellow "Case: bundled binary-mirror-config applies on npmmirror"
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
        if ($LASTEXITCODE -ne 0) { throw "utoo install failed for binary-mirror-config test" }

        if (-not ($bmcOut -match "Bundled binary mirror config:")) {
            throw "bundled binary-mirror-config was never applied (mirror layer skipped)"
        }

        Write-Green "PASS: bundled binary-mirror-config applied"
    }
    finally { Pop-Location }
}
finally {
    Remove-Item -Recurse -Force $bmcDir -ErrorAction SilentlyContinue
}

# ═══════════════════════════════════════════════════════════════
# Case: package-lock.json reuse — adding a dep doesn't redraw the tree
# ═══════════════════════════════════════════════════════════════
# A fresh install writes the baseline lock. Adding one leaf dependency must
# REUSE that lock: every prior package keeps its exact version and `resolved`,
# and the only new entries belong to the added package's own subtree.
Write-Yellow "Case: package-lock.json reuse keeps existing tree stable"
try {
    Push-Location e2e/pm/lockfile-reuse
    Remove-Item -Recurse -Force node_modules, package-lock.json -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$env:USERPROFILE\.cache\nm" -ErrorAction SilentlyContinue
    @{ name = "lockfile-reuse"; version = "1.0.0"; private = $true; dependencies = @{ debug = "4.3.4" } } |
        ConvertTo-Json -Depth 5 | Set-Content "package.json"

    utoo install --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "baseline install failed for lockfile-reuse" }
    Copy-Item package-lock.json package-lock.before.json

    # is-number@7.0.0 has no dependencies of its own (a leaf add).
    utoo install is-number@7.0.0 --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "add is-number failed for lockfile-reuse" }

    $check = @'
const before = require("./package-lock.before.json").packages;
const after = require("./package-lock.json").packages;
for (const [key, pkg] of Object.entries(before)) {
  if (key === "" || pkg.link) continue;
  const now = after[key];
  if (!now) { console.error("REGRESSION: lost lock entry " + key); process.exit(1); }
  if (now.version !== pkg.version) { console.error("REGRESSION: " + key + " version changed"); process.exit(1); }
  if (now.resolved !== pkg.resolved) { console.error("REGRESSION: " + key + " resolved changed"); process.exit(1); }
}
if (!after["node_modules/is-number"]) { console.error("is-number not added to lockfile"); process.exit(1); }
const newKeys = Object.keys(after).filter((k) => !(k in before));
const unexpected = newKeys.filter((k) => k !== "node_modules/is-number");
if (unexpected.length) { console.error("REGRESSION: unrelated entries added: " + unexpected.join(", ")); process.exit(1); }
console.log("lock reuse: " + Object.keys(before).length + " preserved, +" + newKeys.length + " added");
'@
    Set-Content -Path check.js -Value $check
    node check.js
    if ($LASTEXITCODE -ne 0) { throw "adding a dep redrew the existing lock tree" }
    Remove-Item check.js, package-lock.before.json -ErrorAction SilentlyContinue
    Write-Green "PASS: package-lock.json reuse keeps existing tree stable"
}
finally { Pop-Location }

# ═══════════════════════════════════════════════════════════════
# Case: package-lock.json reuse prunes a removed dep's orphaned subtree
# ═══════════════════════════════════════════════════════════════
# Removing a dep must drop it AND any transitive only it pulled in. Baseline
# carries debug (→ ms) and is-odd (→ is-number); removing is-odd prunes both
# is-odd and the orphaned is-number, while debug + ms survive untouched.
Write-Yellow "Case: package-lock.json reuse prunes a removed dep"
try {
    Push-Location e2e/pm/lockfile-reuse
    Remove-Item -Recurse -Force node_modules, package-lock.json -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$env:USERPROFILE\.cache\nm" -ErrorAction SilentlyContinue
    @{ name = "lockfile-reuse"; version = "1.0.0"; private = $true;
        dependencies = @{ debug = "4.3.4"; "is-odd" = "3.0.1" } } |
        ConvertTo-Json -Depth 5 | Set-Content "package.json"

    utoo install --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "baseline install failed for prune case" }
    $base = @'
const p = require("./package-lock.json").packages;
for (const k of ["node_modules/debug","node_modules/ms","node_modules/is-odd","node_modules/is-number"]) {
  if (!p[k]) { console.error("baseline missing " + k); process.exit(1); }
}
'@
    Set-Content -Path check.js -Value $base
    node check.js
    if ($LASTEXITCODE -ne 0) { throw "prune baseline did not contain the expected tree" }
    Copy-Item package-lock.json package-lock.before.json

    # Remove is-odd, reinstall via the reuse path.
    @{ name = "lockfile-reuse"; version = "1.0.0"; private = $true; dependencies = @{ debug = "4.3.4" } } |
        ConvertTo-Json -Depth 5 | Set-Content "package.json"
    utoo install --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "reinstall after removal failed" }
    $check = @'
const before = require("./package-lock.before.json").packages;
const after = require("./package-lock.json").packages;
for (const k of ["node_modules/is-odd", "node_modules/is-number"]) {
  if (after[k]) { console.error("REGRESSION: " + k + " not pruned after removal"); process.exit(1); }
}
for (const k of ["node_modules/debug", "node_modules/ms"]) {
  if (!after[k]) { console.error("REGRESSION: lost " + k); process.exit(1); }
  if (after[k].version !== before[k].version || after[k].resolved !== before[k].resolved) {
    console.error("REGRESSION: " + k + " changed during prune"); process.exit(1);
  }
}
console.log("prune: is-odd + orphaned is-number removed, debug + ms preserved");
'@
    Set-Content -Path check.js -Value $check
    node check.js
    if ($LASTEXITCODE -ne 0) { throw "removing a dep did not prune its orphaned subtree" }
    Remove-Item check.js, package-lock.before.json -ErrorAction SilentlyContinue
    Write-Green "PASS: package-lock.json reuse prunes a removed dep"
}
finally { Pop-Location }

# ═══════════════════════════════════════════════════════════════
# Case: package-lock.json reuse — warm re-install is a faithful no-op
# ═══════════════════════════════════════════════════════════════
# Reseeding the prior tree and re-resolving an unchanged manifest must converge
# to a byte-identical lock — no churn, no reordering, no re-pin.
Write-Yellow "Case: package-lock.json reuse warm re-install is a no-op"
try {
    Push-Location e2e/pm/lockfile-reuse
    Remove-Item -Recurse -Force node_modules, package-lock.json -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$env:USERPROFILE\.cache\nm" -ErrorAction SilentlyContinue
    @{ name = "lockfile-reuse"; version = "1.0.0"; private = $true; dependencies = @{ debug = "4.3.4" } } |
        ConvertTo-Json -Depth 5 | Set-Content "package.json"

    utoo install --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "cold install failed for no-op case" }
    Copy-Item package-lock.json package-lock.cold.json
    utoo install --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "warm reinstall failed for no-op case" }
    if ((Get-Content -Raw package-lock.cold.json) -ne (Get-Content -Raw package-lock.json)) {
        throw "warm re-install changed the lockfile (reuse is not a faithful no-op)"
    }
    Remove-Item package-lock.cold.json, node_modules, package-lock.json -Recurse -Force -ErrorAction SilentlyContinue
    Write-Green "PASS: package-lock.json reuse warm re-install is a no-op"
}
finally { Pop-Location }

# ═══════════════════════════════════════════════════════════════
# Case: bumping a shared dep past a transitive's range stays sound
# ═══════════════════════════════════════════════════════════════
# Baseline hoists is-number@6 (shared by the root and is-odd@3.0.1, which needs
# ^6). Bumping the root's is-number to ^7 collides is-number@7 and the still-
# pinned is-number@6 in one node_modules slot; the resolver must detect that and
# cold-resolve to the npm-correct tree (is-number@7 hoisted, is-number@6 nested
# under is-odd), deterministically across repeats.
Write-Yellow "Case: reuse bump past a transitive's range cold-resolves cleanly"
try {
    Push-Location e2e/pm/lockfile-reuse
    Remove-Item -Recurse -Force node_modules, package-lock.json -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$env:USERPROFILE\.cache\nm" -ErrorAction SilentlyContinue
    @{ name = "lockfile-reuse"; version = "1.0.0"; private = $true; dependencies = @{ "is-number" = "^6.0.0"; "is-odd" = "3.0.1" } } |
        ConvertTo-Json -Depth 5 | Set-Content "package.json"
    utoo install --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "baseline install failed for bump case" }
    node -e "const p=require('./package-lock.json').packages;if(p['node_modules/is-number']?.version?.[0]!=='6'){console.error('baseline did not hoist is-number@6');process.exit(1)}"
    if ($LASTEXITCODE -ne 0) { throw "bump baseline shape wrong" }

    @{ name = "lockfile-reuse"; version = "1.0.0"; private = $true; dependencies = @{ "is-number" = "^7.0.0"; "is-odd" = "3.0.1" } } |
        ConvertTo-Json -Depth 5 | Set-Content "package.json"
    utoo install --ignore-scripts --registry=https://registry.npmjs.org
    if ($LASTEXITCODE -ne 0) { throw "bump reinstall failed" }
    node -e "const p=require('./package-lock.json').packages;const top=p['node_modules/is-number']?.version;const nested=p['node_modules/is-odd/node_modules/is-number']?.version;if(top?.[0]!=='7'){console.error('REGRESSION: root is-number not bumped to 7 (got '+top+')');process.exit(1)}if(nested?.[0]!=='6'){console.error('REGRESSION: is-odd did not nest is-number@6 (got '+nested+')');process.exit(1)}"
    if ($LASTEXITCODE -ne 0) { throw "bump produced an unsound tree" }
    Copy-Item package-lock.json package-lock.bump.json
    foreach ($i in 1..2) {
        utoo install --ignore-scripts --registry=https://registry.npmjs.org | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "repeat install failed for bump case" }
        if ((Get-Content -Raw package-lock.bump.json) -ne (Get-Content -Raw package-lock.json)) {
            throw "bump tree is nondeterministic across installs"
        }
    }
    Remove-Item package-lock.bump.json, node_modules, package-lock.json -Recurse -Force -ErrorAction SilentlyContinue
    Write-Green "PASS: reuse bump past a transitive's range stays sound and deterministic"
}
finally { Pop-Location }

Write-Green "All e2e tests passed successfully!"
