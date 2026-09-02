#!/usr/bin/env bash
# Tests for .claude/hooks/block-push-to-main.sh.
#
# Runs the hook against synthetic Bash tool payloads and asserts the exit code
# (2 = blocked, 0 = allowed). Run from the repo root:
#   bash .claude/hooks/test-block-push-to-main.sh
#
# Defence in depth for CLAUDE.md rule 1: a regex that quietly stops matching
# `develop:main`, `+main` or `--mirror` would leave the rule unenforced while
# still looking installed.

set -uo pipefail

HOOK="$(cd "$(dirname "$0")" && pwd)/block-push-to-main.sh"
failures=0
rc=0

run_hook() {
  local payload
  payload=$(printf '%s' "$1" | python3 -c '
import json, sys
print(json.dumps({"tool_input": {"command": sys.stdin.read()}}))')
  printf '%s' "$payload" | bash "$HOOK" >/dev/null 2>&1
  rc=$?
}

assert_block() {
  run_hook "$1"
  if [ "$rc" -ne 2 ]; then
    echo "FAIL: expected BLOCK, got exit $rc for: $1" >&2
    failures=$((failures + 1))
  fi
}

assert_pass() {
  run_hook "$1"
  if [ "$rc" -ne 0 ]; then
    echo "FAIL: expected PASS, got exit $rc for: $1" >&2
    failures=$((failures + 1))
  fi
}

# The blocked cases are assembled at runtime rather than written out as
# literals: this suite is itself edited through an agent whose Bash calls run
# past the very hook under test, and a literal would trip it.
PUSH="git push"
MAIN="main"

# --- allowed ---
assert_pass "$PUSH -u origin claude/session-b9wznu"
assert_pass "$PUSH origin feature/${MAIN}-menu"
assert_pass "$PUSH origin HEAD:feature"
assert_pass "$PUSH origin v0.1.0"
assert_pass "git fetch --all"
assert_pass "git branch --all"
assert_pass "git log --all --oneline"
assert_pass "git status"
assert_pass "echo do not push to ${MAIN}"

# --- blocked: every refspec whose destination is main ---
assert_block "$PUSH origin ${MAIN}"
assert_block "$PUSH -f origin ${MAIN}"
assert_block "$PUSH --force-with-lease origin ${MAIN}"
assert_block "$PUSH origin HEAD:${MAIN}"
assert_block "$PUSH origin develop:${MAIN}"
assert_block "$PUSH origin :${MAIN}"
assert_block "$PUSH origin +${MAIN}"
assert_block "$PUSH origin refs/heads/${MAIN}"

# --- blocked: global git options between `git` and `push` ---
assert_block "git -C /home/user/shimau push origin ${MAIN}"
assert_block "git -c user.name=x push origin ${MAIN}"

# --- blocked: broadcasts that never name main ---
assert_block "$PUSH --all origin"
assert_block "$PUSH --mirror origin"
assert_block "$PUSH origin --all"

if [ "$failures" -gt 0 ]; then
  echo "$failures assertion(s) failed" >&2
  exit 1
fi
echo "block-push-to-main.sh: all assertions passed"
