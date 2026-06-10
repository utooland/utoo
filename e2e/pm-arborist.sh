#!/usr/bin/env bash
#
# Compatibility wrapper for the migrated arborist fixture e2e suite.
# The assertions live in crates/pm/tests/arborist.rs.

set -euo pipefail

if [ -z "${UTOO_E2E_BIN:-}" ]; then
  UTOO_E2E_BIN="$(command -v utoo)"
  export UTOO_E2E_BIN
fi

export UTOO_RUN_PM_E2E="${UTOO_RUN_PM_E2E:-1}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --all)
      export UTOO_E2E_ARBORIST_ALL=1
      ;;
    --list)
      export UTOO_E2E_LIST=1
      ;;
    *)
      export UTOO_E2E_FILTER="$1"
      ;;
  esac
  shift
done

cargo test -p utoo-pm --test arborist -- --nocapture
