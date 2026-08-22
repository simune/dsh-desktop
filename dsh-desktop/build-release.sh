#!/usr/bin/env bash
# macOS / Linux 入口：一键编译打包 DSH Desktop
set -euo pipefail
cd "$(dirname "$0")"
exec node scripts/build-release.mjs "$@"
