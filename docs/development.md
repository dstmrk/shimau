# Development

Rust 1.85+ and Node 22+. No other toolchain, and nothing to install globally.

## Running the two halves

The backend serves the API and, in production, the built frontend. In
development the two run separately and Vite proxies `/api` across.

```bash
# API on :8080
cd backend
SHIMAU_STACKS_DIR=/path/to/your/stacks \
SHIMAU_DATA_DIR=./data \
SHIMAU_STATIC_DIR=../frontend/dist \
SHIMAU_COOKIE_SECURE=false \
SHIMAU_ADMIN_PASSWORD=dev-password-please \
cargo run
```

```bash
# UI on :5173, proxying /api to :8080
cd frontend && npm install && npm run dev
```

Point `SHIMAU_STACKS_DIR` at a throwaway directory with a stack or two in it,
not at anything you would mind restarting. `SHIMAU_COOKIE_SECURE=false` is
required on `http://localhost`, or the browser drops the session cookie and the
login appears to succeed and bounce straight back.

`SHIMAU_DEV_BACKEND` overrides the proxy target if the API is not on 8080.

## Tests

```bash
cd backend  && cargo test          # unit tests + the HTTP suite
cd frontend && npm run test        # Vitest
```

The HTTP suite in `backend/tests/api.rs` drives the real router without opening
a socket, so it exercises the authentication gate, the path checks and the
file-editing contract exactly as a browser would hit them.

Compose validation tests shell out to `docker compose config`, which needs the
CLI but not a running daemon. Without the CLI they skip themselves rather than
fail, so a machine with no Docker still runs everything else.

## Everything CI runs

```bash
cd backend
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked

cd ../frontend
npm run lint
npm run format:check
npm run typecheck
npm run test:coverage
npm run build

cd ..
node scripts/check-docs.mjs        # every cited path and skill still exists
node scripts/check-contrast.mjs    # theme colour pairs meet contrast
for suite in .claude/hooks/test-*.sh; do bash "$suite"; done
```

`scripts/check-docs.mjs` is the one that surprises people: it fails on a path
cited in prose that no longer exists. A stale map is worse than no map, so a
dead reference fails the build instead of quietly misleading the next reader.

## Where things are

- `docs/spec.md` — the specification the project is built from, including the
  decisions that are settled and the ones that were deliberately left open.
- `docs/architecture/INDEX.md` — the codebase map. A directory tree, a "where
  does X live" table, and the two request paths worth knowing. Read it before
  grepping.
- `docs/security.md` — the security posture and the reasoning behind it.
- `docs/openapi.yaml` — the HTTP surface, for clients that are not the shimau
  frontend. Hand-written; nothing generates or consumes it.
- `CLAUDE.md` and `.claude/skills/` — the rules and the domain knowledge for
  whoever, or whatever, works on this next. The skills are prescriptive (how to
  do X); the map is descriptive (where X is).

## The shape of a change

The backend is small and deliberately boring: one error type, one place that
shells out to Docker, one place that turns a request string into a path, and no
state about your stacks anywhere. If a change wants to add a cache, a second
command builder, or a table describing a stack, it is probably the wrong
change — `docs/spec.md` §16 lists what is settled.

Tests come first for anything with logic, and anything security-sensitive owes
a test for the property that makes it security-sensitive.
