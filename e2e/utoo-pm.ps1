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
Set-Location e2e/pm/ant-design-x  
if (-not (Test-Path "ant-design-x")) {  
    git clone --branch next --single-branch https://github.com/ant-design/x.git ant-design-x  
}  
Set-Location ant-design-x  
Write-Host "Installing dependencies for ant-design-x (next)..."  
utoo deps  
if ($LASTEXITCODE -ne 0) { Write-Red "FAIL: utoo deps failed for ant-design-x (next)"; exit 1 }  
utoo install --ignore-scripts  
if ($LASTEXITCODE -ne 0) { Write-Red "FAIL: utoo install failed for ant-design-x (next)"; exit 1 }  
utoo rebuild  
if ($LASTEXITCODE -ne 0) { Write-Red "FAIL: utoo rebuild failed for ant-design-x (next)"; exit 1 }  
Write-Green "PASS: ant-design-x (next) cloned and installed"  
Set-Location ../../  
  
# Case 2: Clone and install ant-design  
Write-Yellow "Case 2: Clone and install ant-design"  
Set-Location ant-design  
if (-not (Test-Path "ant-design")) {  
    git clone --depth=1 --single-branch https://github.com/ant-design/ant-design.git  
}  
Set-Location ant-design  
Write-Host "Installing dependencies for ant-design..."  
utoo install  
if ($LASTEXITCODE -ne 0) { Write-Red "FAIL: utoo install failed for ant-design"; exit 1 }  
Write-Green "PASS: ant-design cloned and installed"  
Set-Location ../../  
  
# Case 3: antd-test project install  
Write-Yellow "Case 3: antd-test project install"  
Set-Location antd-test  
utoo install  
if (-not (Test-Path "node_modules")) {  
    Write-Red "FAIL: node_modules directory not created"  
    exit 1  
}  
if (-not (Test-Path "node_modules/antd")) {  
    Write-Red "FAIL: antd package not installed"  
    exit 1  
}  
Write-Green "PASS: antd-test install successful"  
Set-Location ..  
  
# Case 4: local-package link test  
Write-Yellow "Case 4: local-package link test"  
Set-Location local-package  
utoo install  
utoo link  
Write-Green "PASS: local-package link successful"  
Set-Location ..  
  
# Case 5: antd-test secondary install  
Write-Yellow "Case 5: antd-test secondary install"  
Set-Location antd-test  
utoo install  
if (-not (Test-Path "node_modules/lodash")) {  
    Write-Red "FAIL: lodash package not installed in secondary update"  
    exit 1  
}  
Write-Green "PASS: antd-test secondary install successful"  
Set-Location ..  
  
# Case 6: antd-test deps tree  
Write-Yellow "Case 6: antd-test deps tree"  
Set-Location antd-test  
utoo deps  
if (-not (Test-Path "package-lock.json")) {  
    Write-Red "FAIL: utoo deps did not generate output"  
    exit 1  
}  
$lockContent = Get-Content package-lock.json -Raw  
if ($lockContent -notmatch "antd") {  
    Write-Red "FAIL: utoo deps output does not contain antd"  
    exit 1  
}  
if ($lockContent -notmatch "react") {  
    Write-Red "FAIL: utoo deps output does not contain react"  
    exit 1  
}  
Write-Green "PASS: antd-test deps tree successful"  
Set-Location ../../..  
  
# Case 7: test global install  
Write-Yellow "Case 7: cowsay global install/uninstall"  
utoo install -g cowsay  
if ($LASTEXITCODE -ne 0) { Write-Red "FAIL: global install cowsay failed"; exit 1 }  
if (-not (Get-Command cowsay -ErrorAction SilentlyContinue)) {  
    Write-Red "FAIL: cowsay not found in PATH after global install"  
    exit 1  
}  
Write-Green "PASS: cowsay global install successful"  
  
# Case 8: reinstall ant-design  
Write-Yellow "Case 8: Clone and install ant-design by npmjs.org"  
Set-Location e2e/pm/ant-design  
git clean -dfx  
Write-Host "Installing dependencies for ant-design by npmjs.org..."  
utoo install --registry=https://registry.npmjs.org  
if ($LASTEXITCODE -ne 0) { Write-Red "FAIL: utoo install failed for ant-design"; exit 1 }  
Write-Green "PASS: ant-design cloned and installed"  
Set-Location ../../  
  
Write-Green "All e2e tests passed successfully!"