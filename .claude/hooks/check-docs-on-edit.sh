#!/usr/bin/env bash
# PostToolUse hook on Edit|Write — revalidates the docs right after a change
# to one of them.
#
# Rationale: CLAUDE.md rule 6. Instead of hoping the next agent remembers to
# run `node scripts/check-docs.mjs` before finishing, a dead path or a
# reference to a skill that no longer exists is reported on the very edit that
# introduced it.

set -euo pipefail

file=$(jq -r '.tool_input.file_path // ""')
[ -z "$file" ] && exit 0

case "$file" in
*/docs/architecture/*.md | docs/architecture/*.md) ;;
*/.claude/skills/*.md | .claude/skills/*.md) ;;
*/CLAUDE.md | CLAUDE.md) ;;
*) exit 0 ;;
esac

# Repo root: two levels up from .claude/hooks/.
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
if ! output=$(cd "$ROOT" && node scripts/check-docs.mjs 2>&1); then
  echo "docs check failed after editing $file (CLAUDE.md rule 6):" >&2
  printf '%s\n' "$output" >&2
  echo "Fix the dead reference above before moving on." >&2
  exit 2
fi

exit 0
