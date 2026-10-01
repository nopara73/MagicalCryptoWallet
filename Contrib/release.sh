#!/usr/bin/env bash
set -euo pipefail
# Unsigned snapshots work without tags. Production signing is a separate step.
ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT_DIR"
case "${1:-snapshot}" in
  snapshot|packages) shift || true; exec python3 Contrib/Releases/package.py "$@" ;;
  production) shift; exec python3 Contrib/Releases/package.py --production "$@" ;;
  *) echo "Usage: $0 snapshot|production --rid <rid> [--version <version>]" >&2; exit 2 ;;
esac
