#!/usr/bin/env bash
# PreToolUse hook on Bash — blocks `git push` whose destination is main.
#
# Rationale: CLAUDE.md rule 1. Everything reaches main through a pull request;
# merging is the maintainer's call. Tag pushes are allowed: they name a tag,
# not main.

set -euo pipefail

cmd=$(jq -r '.tool_input.command // ""')

# Collapse repeated whitespace so the patterns below see the real command.
normalized=$(printf '%s' "$cmd" | tr -s ' ')

# Global git options may sit between `git` and `push` (`-C <dir>`, `-c k=v`,
# `--git-dir=…`), so requiring the two words to be adjacent would leave
# `git -C /repo push origin main` wide open.
GIT_PUSH='git([[:space:]]+-[^[:space:]]+([[:space:]]+[^-[:space:]][^[:space:]]*)?)*[[:space:]]+push'

blocked=""

# Any refspec whose destination is main. The boundary class admits a space,
# `:`, `+` or line start, which covers `develop:main` (push someone else's
# commits), `:main` (delete it), `+main` (force it) and a plain `main`.
# `refs/heads/main` needs naming explicitly because `/` is not in the class.
if printf '%s' "$normalized" | grep -qE "${GIT_PUSH}.*([[:space:]:+]|^)(main|HEAD:main|refs/heads/main)([[:space:]]|\$)"; then
  blocked="refspec"
fi

# `--all` and `--mirror` ship every local branch, main included, without ever
# naming it — and `--mirror` can delete remote refs too. Anchored to a `push`
# so `git fetch --all` and `git branch --all` stay usable.
if printf '%s' "$normalized" | grep -qE "${GIT_PUSH}([[:space:]].*)?[[:space:]](--all|--mirror)([[:space:]]|=|\$)"; then
  blocked="broadcast"
fi

if [ -n "$blocked" ]; then
  echo "Blocked by .claude/hooks/block-push-to-main.sh:" >&2
  if [ "$blocked" = "broadcast" ]; then
    echo "  \`git push --all/--mirror\` ships every local branch, main included" >&2
    echo "  (CLAUDE.md rule 1). Push your branch by name:" >&2
    echo "    git push -u origin <branch-name>" >&2
  else
    echo "  Pushing to main directly is forbidden (CLAUDE.md rule 1)." >&2
    echo "  Open a pull request instead." >&2
  fi
  exit 2
fi

exit 0
