---
name: auth-sessions
description: Use when touching authentication or authorisation — login, logout, the auth middleware, API tokens and their capabilities, password hashing, the login rate limiter, the trusted proxy header, the cookie attributes, the administrator bootstrap, or edits to backend/src/auth/, backend/src/api/auth.rs and backend/src/api/tokens.rs. Covers Argon2id via the argon2 crate defaults, why sessions and API tokens are stored as a SHA-256 and never Argon2id, why no token capability writes a file or mints a token, why the capability check is an extractor rather than a route layer, the SameSite=Lax plus JSON-body CSRF stance, the per-address-and-username backoff curve and how a reverse proxy collapses it, and why the bootstrap never rewrites an existing password.
---

# auth-sessions — one administrator, always on

Authentication is mandatory and there is exactly one account (spec §7.1). No
RBAC, no second user, no OIDC. Everything lives in `backend/src/auth/` and
`backend/src/api/auth.rs`.

Two credentials reach that one account: a session cookie for a browser, and an
API token for anything else. They are not equivalent, and the asymmetry is the
security model rather than an implementation detail.

## Passwords

Argon2id with the crate's default parameters, through
`backend/src/auth/password.rs`. The API is deliberately thin: `hash` produces a
PHC string, `verify` reads one.

`verify` returns `Err` for a malformed stored hash rather than `Ok(false)`. A
corrupt database would otherwise be indistinguishable from a wrong password and
would present as "the password stopped working" forever.

The bootstrap enforces `MIN_PASSWORD_LEN` (12). The rate limiter is a throttle,
not a substitute for a password with entropy in it.

## Sessions

- Token: 32 bytes from the OS CSPRNG (`getrandom`), base64url, no padding.
- The `sessions` table stores **`SHA-256(token)`**, never the token. A leaked
  `shimau.db` then hands out no live sessions.
- Cookie: `HttpOnly`, `SameSite=Lax`, `Path=/`, `Max-Age` from
  `SHIMAU_SESSION_TTL_HOURS`, and `Secure` unless `SHIMAU_COOKIE_SECURE=false`.

`SHIMAU_COOKIE_SECURE=false` exists for plain-HTTP LAN installs. Without it the
browser silently drops the cookie and the user sees a login that "succeeds" and
bounces straight back to the form — the single most confusing failure this app
can produce, which is why the compose file comments it.

## API tokens

`backend/src/auth/token.rs` and `backend/src/api/tokens.rs`. A machine client
presents `Authorization: Bearer shimau_…` instead of the cookie.

- 32 bytes from the OS CSPRNG, base64url, prefixed `shimau_` so the value is
  recognisable in a log and matchable by a secret scanner.
- Stored as `SHA-256`, the same construction as a session token, deliberately
  **not** Argon2id. A password is low-entropy and chosen by a human, so the
  hashing cost is what stands between a stolen hash and a dictionary. A token
  is 256 random bits: there is nothing to guess, and the tens of milliseconds
  would be paid on every request a polling client makes.
- `last_used_at` is rewritten at most once a minute, and the staleness test is
  in the SQL `WHERE` clause so two concurrent requests cannot race it.

### The capability model

`Principal` in `backend/src/api/auth.rs` is the whole authorisation decision:

| Principal | Actions | Writes a file | Manages tokens |
| --- | --- | --- | --- |
| `Session` | yes | yes | yes |
| `Token(read)` | no | no | no |
| `Token(operate)` | yes | no | no |

Two rules here are load-bearing and neither is obvious:

**No capability writes a file.** A machine that can save a Compose file and
then start the stack can give a service `privileged: true` and a bind mount of
`/`, which is root on the host in two calls. `docker compose config` accepts
all of it — it validates syntax, not intent. Adding a capability that writes
files is a product decision, not a convenience.

**No token reaches `/api/tokens`.** A token that could mint a token would be
its own escalation path out of its capability.

### Where the check goes

`require_auth` establishes the principal and nothing more. What that principal
may do is declared by an **extractor in the handler signature** — `Operator`
or `SessionOnly` — not by a route layer.

That is not a style preference. `/compose` and `/env` are a readable GET and a
privileged PUT on one path, and a `route_layer` does not see the method, so a
layer would have to choose between letting a read token write and stopping it
reading. The extractor is also why `Operator` is repeated on all four
lifecycle handlers instead of being hidden in the shared `act` helper: an
extractor only runs when it is in the signature, and a gate that disappears
when someone moves code is not a gate.

When both credentials arrive, the session wins (`authenticate`). Otherwise
attaching a header would downgrade an administrator's request into one a
capability check treats more leniently.

## CSRF

Two gates, no token:

1. `SameSite=Lax` — a cross-site form POST does not carry the cookie.
2. Every mutating endpoint takes a JSON body, which axum's `Json` extractor
   enforces by content type. An HTML form cannot produce `application/json`.

If a mutating endpoint is ever added that takes no body, it needs its own
answer to this; do not assume Lax alone is enough.

## Rate limiting

`backend/src/auth/ratelimit.rs`. Five free attempts, then 2s, 4s, 8s … capped
at 15 minutes, keyed on **client address and username together**:

- keyed on the address alone, one attacked account would lock out every login
  from that address;
- keyed on the username alone, anyone could lock the real administrator out
  from anywhere.

State is in memory on purpose. It is a throttle, not an audit trail, and a
restart is not something an unauthenticated attacker can trigger.

The client address comes from the socket, unless
`SHIMAU_TRUSTED_PROXY_HEADER` names a header to read it from
(`client_address` in `backend/src/api/auth.rs`).

That variable is opt-in and must stay opt-in. The header is spoofable, so
trusting it on an instance that is also reachable directly lets a caller pick
its own limiter key and turns the throttle off. But refusing it outright was
the wrong call, and it was wrong in a way that took a real deployment to see:
behind a tunnel every request shares the proxy's address, which collapses the
per-address half of the key. Six wrong guesses at `admin` from anyone then
hold the real administrator out, for as long as they keep guessing. That is a
denial of service an unauthenticated stranger can run, and "acceptable for a
LAN tool" stopped being true the moment the LAN tool went behind Cloudflare.

The header name is validated at startup (`parse_header_name` in
`backend/src/config.rs`) and lowercased for `HeaderMap` lookup. Only the first
comma-separated value is read: `X-Forwarded-For` accumulates a list as it
crosses proxies and the client is the first entry.

## Bootstrap

`bootstrap_admin` in `backend/src/main.rs` creates the account on first boot
from `SHIMAU_ADMIN_USERNAME` and `SHIMAU_ADMIN_PASSWORD`. Once the account
exists those variables are **ignored**, and the log says so.

That is deliberate: a compose file left with a stale `SHIMAU_ADMIN_PASSWORD`
would otherwise reset the account on every restart, quietly undoing a password
change. `users` has `CHECK (id = 1)`, so a second administrator fails at the
schema.

With no account and no bootstrap password, the process refuses to start with a
message naming the variable. Authentication is mandatory, so a running instance
nobody can log into is worse than a clear failure.

## Failure responses

A wrong username and a wrong password take the same path and produce the same
401, and `credentials_ok` in `backend/src/api/auth.rs` is where that holds.

**The password is verified even when the username does not match**, and the two
answers are combined with `&`, not `&&`. Short-circuiting on the username was
the original shape and it was a username oracle: the wrong name returned in
microseconds, the right one after Argon2id had run for tens of milliseconds.
Identical bodies, different clocks — and knowing the administrator's name is
half of a brute force, on an account whose throttle is keyed by that very name.

The property is tested without a stopwatch: give the account a stored hash that
cannot be parsed and send a wrong username. `verify` returns an error, which is
only reachable if it ran at all — a short-circuit would answer `Ok(false)`.
