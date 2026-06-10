param(
    [switch]$Verbose
)

# Compatibility wrapper for the migrated Windows PM e2e suite.
# The assertions live in crates/pm/tests/e2e.rs.

$ErrorActionPreference = "Stop"

if (-not $env:UTOO_E2E_BIN) {
    $cmd = Get-Command utoo -ErrorAction Stop
    $env:UTOO_E2E_BIN = $cmd.Source
}

if (-not $env:UTOO_RUN_PM_E2E) {
    $env:UTOO_RUN_PM_E2E = "1"
}

cargo test -p utoo-pm --test e2e -- --nocapture
exit $LASTEXITCODE
