#!/usr/bin/env bash
# Tests for .claude/hooks/block-unpinned-actions.sh.
#
# Defence in depth for CLAUDE.md rule 4: the whole point of the hook is that
# nobody has to notice `@v4` by eye during review, so a regex that silently
# stops matching would take the guarantee with it.

set -uo pipefail

HOOK="$(cd "$(dirname "$0")" && pwd)/block-unpinned-actions.sh"
failures=0
rc=0

run_hook() {
  local file="$1" content="$2"
  printf '%s' "$content" | python3 -c '
import json, sys
print(json.dumps({"tool_input": {"file_path": sys.argv[1], "content": sys.stdin.read()}}))' "$file" \
    | bash "$HOOK" >/dev/null 2>&1
  rc=$?
}

# Edit sends `new_string` where Write sends `content`; the hook has to read
# both, so one case goes through that shape too.
run_hook_edit() {
  local file="$1" content="$2"
  printf '%s' "$content" | python3 -c '
import json, sys
print(json.dumps({"tool_input": {"file_path": sys.argv[1], "new_string": sys.stdin.read()}}))' "$file" \
    | bash "$HOOK" >/dev/null 2>&1
  rc=$?
}

assert_block() {
  run_hook "$1" "$2"
  if [ "$rc" -ne 2 ]; then
    echo "FAIL: expected BLOCK, got exit $rc for $1 with: $2" >&2
    failures=$((failures + 1))
  fi
}

assert_pass() {
  run_hook "$1" "$2"
  if [ "$rc" -ne 0 ]; then
    echo "FAIL: expected PASS, got exit $rc for $1 with: $2" >&2
    failures=$((failures + 1))
  fi
}

WORKFLOW=".github/workflows/ci.yml"
PINNED="      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1"

# --- outside the workflow directories: not this hook's business ---
assert_pass "README.md" "uses: actions/checkout@v4"
assert_pass "backend/src/main.rs" "// uses: something@v1"

# --- allowed inside a workflow ---
assert_pass "$WORKFLOW" "$PINNED"
assert_pass "/home/user/shimau/$WORKFLOW" "$PINNED"
assert_pass "$WORKFLOW" "      - uses: ./.github/actions/setup"
assert_pass "$WORKFLOW" "      - uses: docker://alpine:3.20"
assert_pass "$WORKFLOW" "      - name: Build
        run: cargo build"
assert_pass ".github/actions/setup/action.yml" "$PINNED"

# --- blocked: mutable refs ---
assert_block "$WORKFLOW" "      - uses: actions/checkout@v4"
assert_block "$WORKFLOW" "      - uses: actions/checkout@main"
assert_block "$WORKFLOW" "      - uses: some/action@v1.2.3 # tempting"
assert_block "$WORKFLOW" "        uses: some/action@abc123"
# A short SHA is not a pin either: it is ambiguous and unreviewable.
assert_block "$WORKFLOW" "      - uses: actions/checkout@3d3c42e"
# One good line does not excuse a bad one further down.
assert_block "$WORKFLOW" "$PINNED
      - uses: actions/setup-node@v4"

# --- the Edit shape reaches the same verdict ---
run_hook_edit "$WORKFLOW" "      - uses: actions/checkout@v4"
if [ "$rc" -ne 2 ]; then
  echo "FAIL: expected BLOCK through new_string, got exit $rc" >&2
  failures=$((failures + 1))
fi
run_hook_edit "$WORKFLOW" "$PINNED"
if [ "$rc" -ne 0 ]; then
  echo "FAIL: expected PASS through new_string, got exit $rc" >&2
  failures=$((failures + 1))
fi

if [ "$failures" -gt 0 ]; then
  echo "$failures assertion(s) failed" >&2
  exit 1
fi
echo "block-unpinned-actions.sh: all assertions passed"
