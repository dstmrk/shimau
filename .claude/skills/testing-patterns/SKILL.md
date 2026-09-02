---
name: testing-patterns
description: Use when writing or fixing tests — Rust unit tests, the HTTP integration suite in backend/tests/api.rs, Vitest tests under frontend/src, or the shell suites for the hooks in .claude/hooks. Covers the harness that drives the real router without a network listener, why some tests need the Compose CLI and how they behave without it, testing the rate limiter without sleeping, what every security-sensitive module owes a test, and the coverage exclusions.
---

# testing-patterns — what gets tested and how

Spec §15.3 lists what the suite must cover. This is how each of those is
actually reached.

## Backend: unit tests next to the code

Every module carries its own `#[cfg(test)] mod tests`. The ones that exist
because a regression there would be a security bug, not an inconvenience:

- `stacks::paths` — traversal, separators, dotfiles, symlink escapes, a file
  where a directory is expected.
- `stacks::files` — commit, drop-without-commit, backup content, `0600` on
  `.env` and its backup, mode inheritance.
- `stacks::discovery` — all four Compose filenames, none, duplicates, `.env`
  present and absent, a *directory* named `compose.yaml`.
- `compose` — the action-to-command mapping, that no action expands to `down`,
  and that no `SHIMAU_*` variable reaches a child process.
- `compose::status` — both `ps --format json` shapes, unhealthy containers,
  malformed output as an error rather than a silent empty list.
- `auth::password` / `auth::session` / `auth::ratelimit` — hashing, cookie
  attributes, cookie parsing, the backoff curve.

## Backend: the HTTP suite

`backend/tests/api.rs` drives the real `Router` with
`tower::ServiceExt::oneshot` — no listener, no ports, no flakiness. The
harness builds an `AppState` over an in-memory database and a `tempfile`
stacks directory, so each test gets its own filesystem.

`ConnectInfo` is inserted into the request extensions by hand, because nothing
here goes through `into_make_service_with_connect_info`.

The suite is where the cross-cutting promises live: every stack endpoint
returns 401 without a session, traversal is rejected over real HTTP including
percent-encoded forms, an invalid Compose body cannot overwrite the file, and
the `.env` round trip keeps its permissions.

## Tests that need the Compose CLI

`docker compose config` validates without a daemon, so validation tests run
anywhere the CLI is installed — including CI, where the runner ships it. They
check `docker_compose_available()` and print a skip line otherwise, so a
developer without Docker still gets a green suite instead of a red one they
cannot fix.

There is no test that needs a running daemon. If you add one, it belongs
behind a disposable Compose project under `#[ignore]`, never against whatever
stacks the developer happens to have.

## Time-dependent tests never sleep

`LoginLimiter` has `retry_after_at` / `record_failure_at` taking an `Instant`,
so the backoff and the TTL are tested by passing a later instant. A test that
sleeps for the delay is a test that will be flaky on a loaded runner.

## Frontend

Vitest with jsdom and Testing Library. `frontend/src/test/setup.ts` stubs
`matchMedia`, which jsdom lacks and the theme provider reads on mount.

Component tests query by role and accessible name, which keeps them honest
about the UI actually being operable. `stack-card.test.tsx` is the example
worth copying: it asserts the behaviour the spec requires (Start replaces Stop
when the stack is down, `.env` is hidden when the file is absent, an ambiguous
stack offers no actions) rather than the markup.

`fetch` is stubbed with `vi.stubGlobal` and `vi.fn<typeof fetch>()` — the type
parameter is what makes `mock.calls[0][0]` typed, and the build type-checks
tests too.

**Do not try to type into CodeMirror under jsdom.** It calls
`getClientRects()` during measurement, which jsdom does not implement, and the
failure surfaces as an unhandled error from an animation frame rather than a
clean assertion. Test what surrounds the editor — that the file is fetched
once, that the filename is shown — and leave the save path to
`backend/tests/api.rs`, where it is exercised against a real
`docker compose config`.

**A modal makes the rest of the page unreachable by role.** A button rendered
outside the dialog is inside an `aria-hidden` subtree, so `getByRole` will not
find it. To force a parent re-render, use `rerender()` from `render()` rather
than clicking something.

Coverage excludes `frontend/src/components/ui/` (generated) and
`frontend/src/main.tsx` (a three-line bootstrap).

## Hook suites

`.claude/hooks/test-*.sh` run the hooks against synthetic tool payloads and
assert the exit code, and CI runs them. They build their blocked commands from
variables instead of literals: these files are themselves edited through an
agent whose Bash calls run past the very hook under test, and a literal
`git push origin main` in the source would block the edit.
