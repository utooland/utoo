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

Write-Green "All e2e tests passed successfully!"