---
name: file-safety
description: Use when changing how shimau writes files — the Compose editor, the .env editor, backups, validation ordering, file permissions, or edits to backend/src/stacks/files.rs and the read/write handlers in backend/src/api/stacks.rs. Covers the stage-validate-commit sequence, why the backup is a copy rather than a rename, why the staging filename is scrubbed out of validator output, the 0600 mode on .env and its backup, and the rules about never logging secrets.
---

# file-safety — the file on disk is old or new, never half-written

Editing a Compose file is privileged: a truncated write takes a stack down and
loses the only copy of its configuration. `backend/src/stacks/files.rs`
implements one primitive, `AtomicWrite`, and both editors go through it.

## Stage, validate, commit

```text
1. stage    write the candidate to .shimau-staged-<pid>-<nonce>-<name>
            in the target's own directory, fsync it
2. validate docker compose --file <staged> config --quiet, cwd = stack dir
3. commit   copy the current file to <name>.bak, then rename(staged, target)
```

The order is the whole point. Validation happens on a file the stack is not
using, so a rejected candidate leaves the original untouched — `AtomicWrite`
removes the staged file on `Drop`, which is what runs when the handler returns
the 422.

The backup is a **copy**, not a rename. Renaming the original aside first would
leave a window where the Compose file does not exist at all; a copy means the
target is only ever the old content or the new one.

The staged file lives in the target's directory so the final step is a
`rename(2)` on one filesystem, which is atomic. Staging in `/tmp` would make it
a cross-device copy and lose the guarantee.

The staging name is not one of the four supported Compose filenames, so a
concurrent scan never mistakes it for a stack's Compose file.

## Scrub the staging name out of errors

`docker compose config` names the file it was handed. That is the staging
filename, which is an implementation detail the user cannot act on.
`sanitize_compose_output` rewrites it back to the real filename before the
error leaves the process. Keep that call if you touch the validation path.

## `.env` is a text file and nothing else

It is written back byte for byte: no parsing, no normalisation, no reordering
(spec §4.6). Both the file and its backup get mode `0600`, and there is a test
asserting it — a world-readable `.env.bak` would be a secret leak created by
the act of editing.

The Compose file inherits the mode of the file it replaces, falling back to
`0644`, so an operator who tightened permissions on a Compose file does not
have them widened by saving from the UI.

## Secrets never reach the logs

`.env` content is never logged, at any level. The write handler logs the stack
name and nothing else. `ApiError::Internal` logs its context server-side and
returns a bare "internal error" to the client, because that context can name
filesystem paths.

If you add a log line anywhere near the `.env` path, the question to answer is
not "is this level low enough" — it is whether the value can appear at all.

## Editing without a `.env`

`PUT /api/stacks/{stack}/env` answers 404 when the file does not exist. That
follows the spec: `.env` is optional, shown only when present, and shimau does
not create stack configuration that the operator did not put there. Creating
one from the UI would be a scope change, not a bug fix.

## Size limits

`MAX_EDITABLE_BYTES` (1 MiB) caps both directions, and the router installs a
matching body limit. Reading applies it too, via `metadata().len()`, so a
pathological file cannot be pulled into memory before the check.
