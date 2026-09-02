#!/usr/bin/env bash
# PreToolUse hook on Bash — blocks `git commit` while main is checked out.
#
# Rationale: CLAUDE.md rule 1. Blocking only the push leaves a commit stranded
# on main that then has to be moved by hand; refusing at commit time keeps the
# branch clean in the first place.

set -euo pipefail

cmd=$(jq -r '.tool_input.command // ""')
normalized=$(printf '%s' "$cmd" | tr -s ' ')

# Same tolerance for global git options as block-push-to-main.sh.
if printf '%s' "$normalized" | grep -qE 'git([[:space:]]+-[^[:space:]]+([[:space:]]+[^-[:space:]][^[:space:]]*)?)*[[:space:]]+commit([[:space:]]|$)'; then
  # Empty output means a detached HEAD (rebase, bisect), which is not "on main".
  branch=$(git branch --show-current 2>/dev/null || true)
  if [ "$branch" = "main" ]; then
    echo "Blocked by .claude/hooks/block-commit-on-main.sh:" >&2
    echo "  Committing on main is forbidden (CLAUDE.md rule 1)." >&2
    echo "  Branch first: git checkout -b <branch-name>" >&2
    exit 2
  fi
fi

exit 0
