# CLAUDE.md — shimau

## The project

shimau is a small self-hosted web application for managing **Docker Compose
projects**. It is not a general-purpose Docker platform: it is a thin, reliable
UI over the Compose CLI, aimed at people who already manage their applications
as `compose.yaml` files and want something simpler than Dockge.

Compose files are the source of truth. The filesystem is the source of truth.
Docker Compose orchestrates. shimau adds as little state as it can get away
with — one SQLite file holding an administrator account and its sessions, and
nothing else.

**Stack:** Rust · Axum · Tokio · rusqlite · Argon2id — React 19 · TypeScript ·
Vite · Tailwind 4 · shadcn/ui · TanStack Query · CodeMirror. One container,
published to `ghcr.io/dstmrk/shimau` on every merge to `main`.

The product specification the repository is built from is in
`docs/spec.md`; the codebase map is in `docs/architecture/INDEX.md`. Version
lives in `backend/Cargo.toml`.

## Guiding principles

- **Compose-first.** Manage projects, not individual containers. A feature that
  only makes sense for a bare container is out of scope.
- **Filesystem-first.** Never import stack configuration into a database. If
  Docker or the filesystem can answer a question, do not cache the answer.
- **Minimal surface area.** The smallest implementation that satisfies the
  current requirement *entirely* — no speculative abstraction, no configuration
  for a hypothetical deployment. But the architectural choice is made for the
  long run: never a stopgap meant to be redone.
- **Safe by default.** No arbitrary shell, no arbitrary Docker command, no web
  terminal. Every capability is a named operation.
- **Reproducible.** Anything the UI does must be reproducible by hand from the
  stack directory, and the operation output shows the command.
- **Modern UI, boring backend.**

## Read the map before exploring

Before grepping the tree, read `docs/architecture/INDEX.md`: the directory
tree, a "where does X live" table, the request paths, and the cross-cutting
modules. The skills are *prescriptive* (how to do X); the map is *descriptive*
(where X is).

## Always-active rules

Twelve. They are here because they are needed **before** you know you need
them — no skill activates in time to save you. Everything else lives in the
skill or the gate that owns it. The numbering is stable and never recycled.

- **1 · Always a branch.** Never commit or push to `main`. Always a pull
  request; merging is the maintainer's call unless they say otherwise.
  Gate: `.claude/hooks/block-push-to-main.sh`,
  `.claude/hooks/block-commit-on-main.sh`.
- **2 · Test first.** The test before the implementation. Every module with
  logic has tests; every security-sensitive module has tests for the property
  that makes it security-sensitive. → skill `testing-patterns`.
- **3 · On ambiguity, propose — do not ask into the void.** An open question
  ("how do you want it?") hands the work of imagining the answer back to the
  user. Give two or three concrete options, say which you would pick and why,
  one trade-off line each. One question at a time. And never ask what the
  repository already answers — read it.
- **4 · Every action pinned to a commit SHA.** A tag is mutable, which is how a
  compromised action reaches a workflow holding `packages: write`. Full
  40-character SHA, human-readable version in a trailing comment.
  Gate: `.claude/hooks/block-unpinned-actions.sh`.
- **5 · Scope discipline.** Before implementing, check the feature is in the
  spec. Do not add arbitrary Docker commands, extra resource managers, a
  scheduler, notifications, metrics, multi-user support, or `docker compose
  down`. A helpful addition nobody asked for is still scope creep, and in this
  application it is also attack surface.
- **6 · Documentation references stay alive.** Every path and skill cited in
  `CLAUDE.md`, `docs/`, or a skill must still exist.
  Gate: `scripts/check-docs.mjs`, run in CI and by
  `.claude/hooks/check-docs-on-edit.sh`.
- **7 · The lesson goes in the skill, then in the gate.** After solving
  something non-trivial with a reusable lesson — a debugging pattern, a setup
  trap, a wrong assumption — write it down immediately, in the **skill** that
  owns the domain, not here: `CLAUDE.md` holds only what is needed before a
  skill can activate. This applies just as much when the user corrects you.
  Then close the loop: prose is a reminder you pay for in context on every
  task, so push it towards a hook or a test, and once the gate exists, cite the
  gate and drop the prose.
- **8 · Enumerate the edge cases.** After each implementation, list them and add
  the tests that cover them, before committing.
- **9 · No generic execution.** There is no endpoint that takes a command, a
  subcommand, an image or a flag from the client, and there never will be. A
  new capability is a new enum variant with its own typed handler.
  → skill `compose-execution`.
- **10 · Nothing from a request becomes a path.** The only client-supplied
  identifier that reaches the filesystem is a stack name, and it goes through
  `backend/src/stacks/paths.rs`. Filenames come from a fixed set in the
  handler. → skill `path-security`.
- **11 · Secrets never reach a log.** `.env` content is never logged at any
  level; internal error context is logged server-side and never returned;
  Compose child processes get an environment allowlist, not ours.
  → skill `file-safety`.
- **12 · Hand over the ledger, not the diff.** The pull request body carries the
  decisions made where the spec was silent, least-confident first, and the
  cost: lines by kind plus every new structural surface. The repository
  squash-merges, so that text becomes the commit message.
  → skill `decision-ledger`.

## Before opening a pull request

```bash
# Backend
cd backend
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked

# Frontend
cd ../frontend
npm run lint
npm run format:check
npm run typecheck
npm run test:coverage
npm run build

# Repository
cd ..
node scripts/check-docs.mjs
for suite in .claude/hooks/test-*.sh; do bash "$suite"; done
```

Then three passes, in this order:

1. **Shape.** Refactor while the code is fresh and changing it is cheap:
   development shims, duplicated concepts, parallel abstractions, compatibility
   wrappers introduced by this work — collapse them into one contract.
2. **Diff.** Only now, re-read the diff adversarially against stable code. If a
   fix reopens the shape, go back to pass 1; you are done when a full pass
   finds nothing.
3. **Docs.** `docs/architecture/INDEX.md` (rule 6), the skill that learned
   something (rule 7).

Finally the decision ledger and the cost table (rule 12).

## Skills

Under `.claude/skills/`. They activate on their own when the task matches their
description; the list is verified by `scripts/check-docs.mjs`, so a new skill
that nothing cites fails CI.

- skill `path-security` — turning a request string into a path, safely.
- skill `compose-execution` — the closed action set, the environment
  allowlist, status parsing.
- skill `auth-sessions` — Argon2id, session cookies, login backoff, bootstrap.
- skill `file-safety` — atomic saves, validation order, backups, `.env`
  permissions.
- skill `sse-streaming` — replay-then-follow, event types, killing children.
- skill `frontend-patterns` — React 19 rules, the shadcn preset, lazy editors.
- skill `testing-patterns` — what the suites cover and how they avoid flakiness.
- skill `release-deploy` — the multi-arch image, pinned Compose CLI, GHCR tags.
- skill `decision-ledger` — the pull request body format.

## Hooks

Under `.claude/hooks/`, wired in `.claude/settings.json`. Each has a suite
(`.claude/hooks/test-*.sh`) that CI runs.

- `.claude/hooks/block-push-to-main.sh` — blocks a push whose destination is
  `main`, including `develop:main`, `:main`, `+main` and `--mirror` (rule 1).
- `.claude/hooks/block-commit-on-main.sh` — blocks a commit while `main` is
  checked out (rule 1).
- `.claude/hooks/block-unpinned-actions.sh` — blocks a workflow edit that
  introduces an action pinned to a tag or branch (rule 4).
- `.claude/hooks/check-docs-on-edit.sh` — revalidates the docs on the edit that
  changed them (rule 6).
