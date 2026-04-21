#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"

major="${VERSION_MAJOR:-1}"
commit_count="${VERSION_COMMIT_COUNT:-}"

if [[ -z "$commit_count" ]]; then
  if git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    commit_count="$(git -C "$REPO_ROOT" rev-list --count HEAD)"
  else
    commit_count=0
  fi
fi

minor=$((commit_count / 100))
patch=$((commit_count % 100))

printf '%s.%02d.%02d\n' "$major" "$minor" "$patch"
