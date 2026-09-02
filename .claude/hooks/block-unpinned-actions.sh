#!/usr/bin/env bash
# PreToolUse hook on Edit|Write — refuses a workflow step that pins a
# third-party action to a tag or branch instead of a commit SHA.
#
# Rationale: CLAUDE.md rule 4. A tag is mutable, so `@v4` means "whatever that
# tag points at the next time CI runs" — which is how a compromised action
# reaches a workflow holding `packages: write`. Reviewing this by eye fails
# exactly once, silently.
#
# Local composite actions (`uses: ./.github/actions/...`) and Docker image
# references (`uses: docker://...`) are exempt: neither resolves through a
# GitHub ref.

set -euo pipefail

payload=$(cat)
file=$(printf '%s' "$payload" | jq -r '.tool_input.file_path // ""')

case "$file" in
*/.github/workflows/*.y*ml | .github/workflows/*.y*ml) ;;
*/.github/actions/*.y*ml | .github/actions/*.y*ml) ;;
*) exit 0 ;;
esac

# Write carries `content`; Edit carries `new_string`. Both are the text about
# to land in the file.
content=$(printf '%s' "$payload" | jq -r '.tool_input.content // .tool_input.new_string // ""')
[ -z "$content" ] && exit 0

offenders=$(printf '%s\n' "$content" \
  | grep -nE '^[[:space:]]*(-[[:space:]]+)?uses:[[:space:]]*[^[:space:]]+' \
  | grep -vE 'uses:[[:space:]]*\./' \
  | grep -vE 'uses:[[:space:]]*docker://' \
  | grep -vE 'uses:[[:space:]]*[^[:space:]]+@[0-9a-f]{40}([[:space:]]|$)' \
  || true)

if [ -n "$offenders" ]; then
  echo "Blocked by .claude/hooks/block-unpinned-actions.sh:" >&2
  echo "  Every third-party action must be pinned to a full 40-character commit SHA" >&2
  echo "  (CLAUDE.md rule 4), with the human-readable version in a trailing comment:" >&2
  echo "    uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1" >&2
  echo "  Resolve one with:" >&2
  echo "    git ls-remote https://github.com/<owner>/<repo> refs/tags/<tag>" >&2
  echo "  Unpinned:" >&2
  printf '%s\n' "$offenders" | sed 's/^/    /' >&2
  exit 2
fi

exit 0
