---
name: path-security
description: Use when any code turns a browser-supplied string into a filesystem path — a stack name from a route parameter, a Compose or .env filename, a backup or staging path, a new API endpoint under /api/stacks, or a change to backend/src/stacks/paths.rs or backend/src/stacks/discovery.rs. Covers the two-gate model (character allowlist then canonicalisation against the stacks root), why symlink escapes need the second gate, why dotfiles are refused, why a traversal attempt answers 400 rather than 404, and which properties must never regress.
---

# path-security — the browser must never name a path

Every filesystem operation in shimau starts from one client-supplied string: a
stack name. `backend/src/stacks/paths.rs` is the only place that turns it into
a path, and nothing else may join a request value onto a directory.

## The two gates

Both run, always, in this order. Either alone has a known hole.

1. **Character allowlist** (`validate_name`). ASCII alphanumerics plus `_`,
   `-` and `.`; not `.` or `..`; no leading dot; at most 128 characters. This
   rejects `..`, `/`, `\`, NUL and anything a URL decoder turns into a
   separator, before touching the disk.
2. **Canonicalisation** (`resolve`). Join onto the canonical stacks root,
   `canonicalize()`, then require the result to be inside the root and to be a
   directory.

Gate 1 alone misses a **symlink inside the root pointing outside it**: the name
`escape` is perfectly legal and the path still leaves the stacks directory.
Gate 2 alone misses nothing on Linux but leans entirely on `canonicalize()`
semantics for a hostile string; keeping the cheap check in front means the
expensive syscall never sees one.

`resolve_file` applies the same reasoning inside a stack directory. It exists
because a `compose.yaml` can itself be a symlink to `/etc/shadow`, and the
editor would happily read and write through it.

## Why a leading dot is refused

`.git`, `.ssh`, `.env` — accepting a dotfile name would create a path
reachable by API but absent from the listing, because `discovery::scan` skips
the same names. A path the UI cannot show and the API can act on is exactly
the asymmetry an attacker looks for.

## Why traversal answers 400, not 404

`map_path_error` in `backend/src/api/stacks.rs` maps `NotFound` and
`NotADirectory` to 404, and everything else — `IllegalCharacter`, `Escapes`,
`RelativeSegment` — to 400. A 404 would confirm or deny the existence of a path
outside the stacks directory. A 400 says only "that is not a stack name".

## Properties that must not regress

Each has a test in `backend/src/stacks/paths.rs`; if you touch this module,
these are the assertions to read first:

- `../`, `../../etc/passwd`, `/etc/passwd`, `foo/bar`, `foo\bar`, NUL and the
  empty string are all rejected by name alone.
- A symlinked stack directory pointing outside the root resolves to `Escapes`.
- A symlinked `compose.yaml` pointing outside the stack resolves to `Escapes`.
- A regular file where a stack directory is expected resolves to
  `NotADirectory`, not to a readable "stack".
- A file that does not exist yet (a new `.env`, a `.bak`) is allowed, because
  its parent is already canonical.

`backend/tests/api.rs` repeats the traversal cases over real HTTP, including
percent-encoded forms, because the router's own decoding sits between the
browser and this module.

## When adding an endpoint

Take the stack name as a path parameter and pass it through
`stacks::resolve_stack`. Never accept a path, a filename, or a directory in a
request body. If a new endpoint needs a file inside a stack, name that file
from a fixed set in the handler and run it through `paths::resolve_file` —
including files shimau creates itself.
