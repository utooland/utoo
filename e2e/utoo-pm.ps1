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
    # Use --ignore-scripts to skip prepare hook that causes @swc/core native binding issues on Windows
    utoo install --registry=https://registry.npmjs.org --ignore-scripts
    if ($LASTEXITCODE -ne 0) { throw "utoo install failed for ant-design (npmjs.org)" }
    
    Write-Green "PASS: ant-design cloned and installed"
}
finally {
    Pop-Location
}

Write-Green "All e2e tests passed successfully!"