---
name: auth-sessions
description: Use when touching authentication — login, logout, the session middleware, password hashing, the login rate limiter, the cookie attributes, the administrator bootstrap, or edits to backend/src/auth/ and backend/src/api/auth.rs. Covers Argon2id via the argon2 crate defaults, why the session table stores a SHA-256 of the token, the SameSite=Lax plus JSON-body CSRF stance, the per-address-and-username backoff curve, and why the bootstrap never rewrites an existing password.
---

# auth-sessions — one administrator, always on

Authentication is mandatory and there is exactly one account (spec §7.1). No
RBAC, no second user, no OIDC. Everything lives in `backend/src/auth/` and
`backend/src/api/auth.rs`.

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

The client address comes from the socket. Behind a reverse proxy every request
arrives from the proxy, which for a single-admin LAN tool is acceptable — but
do not "fix" it by trusting `X-Forwarded-For`: an unauthenticated, spoofable
header would turn the limiter off entirely.

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
