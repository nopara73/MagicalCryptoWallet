#!/usr/bin/env bash
set -euo pipefail

# Python owns isolated data directories, bounded RPC checks and only its own child processes.
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
exec python3 "$script_dir/test-single-wallet-coinjoin.py" "$@"
