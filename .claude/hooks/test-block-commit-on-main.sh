#!/usr/bin/env bash
# Tests for .claude/hooks/block-commit-on-main.sh.
#
# The hook's decision depends on the checked-out branch, so every case runs
# inside a throwaway repository created on a known branch.

set -uo pipefail

HOOK="$(cd "$(dirname "$0")" && pwd)/block-commit-on-main.sh"
failures=0
rc=0

run_hook_on_branch() {
  local branch="$1" cmd="$2" repo
  repo=$(mktemp -d)
  (
    cd "$repo" || exit 1
    git init -q -b "$branch" .
    git config user.email test@example.com
    git config user.name test
    printf '%s' "$cmd" | python3 -c '
import json, sys
print(json.dumps({"tool_input": {"command": sys.stdin.read()}}))' \
      | bash "$HOOK" >/dev/null 2>&1
  )
  rc=$?
  rm -rf "$repo"
}

assert_block() {
  run_hook_on_branch "$1" "$2"
  if [ "$rc" -ne 2 ]; then
    echo "FAIL: expected BLOCK on branch $1, got exit $rc for: $2" >&2
    failures=$((failures + 1))
  fi
}

assert_pass() {
  run_hook_on_branch "$1" "$2"
  if [ "$rc" -ne 0 ]; then
    echo "FAIL: expected PASS on branch $1, got exit $rc for: $2" >&2
    failures=$((failures + 1))
  fi
}

COMMIT="git commit"

# --- allowed: anywhere but main ---
assert_pass feature "$COMMIT -m 'feat: something'"
assert_pass feature "$COMMIT --amend --no-edit"
assert_pass feature "git -C . commit -m x"

# --- allowed on main: not a commit ---
assert_pass main "git status"
assert_pass main "git log --oneline"
assert_pass main "git checkout -b feature"
assert_pass main "echo about to commit"

# --- blocked: a commit while main is checked out ---
assert_block main "$COMMIT -m 'fix: oops'"
assert_block main "$COMMIT --amend"
assert_block main "git -C . commit -m x"
assert_block main "git -c user.name=x commit -m y"
assert_block main "git add -A && $COMMIT -m wip"

if [ "$failures" -gt 0 ]; then
  echo "$failures assertion(s) failed" >&2
  exit 1
fi
echo "block-commit-on-main.sh: all assertions passed"
