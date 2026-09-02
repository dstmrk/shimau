---
name: sse-streaming
description: Use when working on live output — the operation console, the log viewer, the SSE endpoints in backend/src/api/operations.rs and backend/src/api/stacks.rs, the operation registry in backend/src/ops/mod.rs, or the browser side in frontend/src/hooks/use-event-stream.ts. Covers the replay-then-follow contract, the named event types, why the child process is killed when the client disconnects, how a reloaded page re-attaches to a running operation, and what happens when a subscriber falls behind.
---

# sse-streaming — replay, then follow

Long operations show their output, not a spinner (spec §5.2). The transport is
Server-Sent Events: one direction, plain HTTP, no second protocol to secure.

## Events

Three named events, all with a JSON payload:

| event      | payload                                | meaning                       |
| ---------- | -------------------------------------- | ----------------------------- |
| `line`     | `{"stream":"stdout\|stderr","text":…}` | one line of output            |
| `finished` | operation: `{"status",…}`; logs: `{"exit_code"}` | the stream is over   |
| `lagged`   | `{"skipped":N}`                        | the browser fell behind       |

The browser closes the `EventSource` on `finished`. Without that, EventSource
reconnects on its own and replays the entire transcript from the top.

## Replay then follow, under one lock

`Operation::subscribe()` returns the buffered snapshot *and* a live receiver
taken under the same mutex. Doing it in two steps would drop any line printed
between the snapshot and the subscription — the bug you would only see under
load, on the one line that mattered.

An operation that has already finished yields its transcript and a `finished`
event, then closes. Opening the console late, or reloading mid-`update`, shows
the whole run either way.

## Re-attaching after a reload

`GET /api/stacks` carries `active_operation_id` for any stack with something
in flight. The dashboard uses it for two things: disabling that stack's action
buttons, and letting a fresh page re-attach to a run this browser never
started. Operations are keyed per stack in the registry, so a second action on
a busy stack is a 409, not a race.

## Backpressure and bounds

- The broadcast channel holds 256 events; a subscriber that falls behind gets
  `RecvError::Lagged`, which becomes a `lagged` event rather than a
  disconnection. The operation is still live, so the stream continues.
- The replay buffer holds 2000 lines and drops the oldest, setting
  `truncated`. It is a reconnection buffer, not an archive.
- Finished operations are evicted once 50 have accumulated, oldest finished
  first; running ones are never evicted.

## Killing the child

`compose::spawn_lines` sets `kill_on_drop(true)`. When the browser closes a log
stream, axum drops the response body, which drops the `LineStream`, which kills
`docker compose logs --follow`. Without it every opened log dialog would leak a
process for the lifetime of the container.

## The browser renders a terminal, not a byte stream

The API relays what Docker printed, verbatim; `frontend/src/lib/terminal.ts`
turns that into what a terminal would have shown, and `ConsoleOutput` is the
only place it is called.

- **Escape sequences.** `--no-color` silences Compose's service prefix and
  nothing else: a container colours its own output, so a Homebridge line
  arrives as `ESC[36m[Nuki]ESC[39m …` and renders in a `<pre>` as literal
  `[36m` noise. Colour, cursor and erase sequences, operating-system commands
  and stray control characters are stripped; a carriage return keeps only the
  frame after it, because that is all the terminal ever showed.
- **Progress redraws.** `--progress plain` prints every tick on its own line,
  so one `pull` produces a hundred `d21668d1c7b3 Extracting 61.83MB` lines.
  `collapseProgress` folds them back onto one row per layer or container,
  keyed on everything left of a status word from a closed list —
  `Warning` and `Error` are not on it, so a failure is never overwritten.

Collapsing is opt-in and only `OperationConsole` sets it. Container logs are
arbitrary text: two identical-looking lines there are two events, and the log
viewer shows both.

Neither transform reaches the replay buffer, so a 2000-line buffer still fills
with raw progress ticks during a long `pull`. Collapsing at the source would
mean parsing Compose output in the backend as well — the same knowledge in two
languages, for a transcript nobody reads after the run.

## A stopped stack is not an error

`docker compose logs --follow` on a stack with no containers returns what
exists and exits. The UI reports "the stream ended" rather than an error
(spec §4.4).

## Never leave the UI spinning

`OperationConsole` falls back to `GET /api/operations/{id}` if the stream drops
before a `finished` event — a proxy timing out, a container restart. A failed
operation must not leave the frontend stuck in a loading state (spec §11), and
the reconnect path is how that promise is kept.
