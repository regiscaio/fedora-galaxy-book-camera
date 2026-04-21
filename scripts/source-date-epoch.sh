#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

epoch="${SOURCE_DATE_EPOCH_OVERRIDE:-}"

if [[ -n "$epoch" ]]; then
  printf '%s\n' "$epoch"
  exit 0
fi

if git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  git -C "$REPO_ROOT" log -1 --format=%ct
else
  printf '0\n'
fi
