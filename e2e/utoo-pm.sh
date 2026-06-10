#!/usr/bin/env bash
#
# Compatibility wrapper for the migrated PM e2e suite.
# The assertions live in crates/pm/tests/e2e.rs.

set -euo pipefail

if [ -z "${UTOO_E2E_BIN:-}" ]; then
  UTOO_E2E_BIN="$(command -v utoo)"
  export UTOO_E2E_BIN
fi

export UTOO_RUN_PM_E2E="${UTOO_RUN_PM_E2E:-1}"

cargo test -p utoo-pm --test e2e -- --nocapture
