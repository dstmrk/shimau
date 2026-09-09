# Security

shimau controls Docker, which makes it an administrative application. Anyone
who can reach it and sign in can run containers on the host, and a container
can be given the host. Treat it as one.

This page is what shimau does about that, and what it leaves to you.
`docs/spec.md` §7 is the specification the code is held to;
`.claude/skills/` carries the reasoning for whoever changes it next.

## Authentication

**Mandatory, and it cannot be turned off.** There is no configuration that
disables it, and there never will be.

One local administrator, created on first boot from `SHIMAU_ADMIN_PASSWORD`
and ignored afterwards, so a stale password left in a compose file cannot
silently reset the account. Passwords are Argon2id, never stored in the clear.

The session cookie is `HttpOnly`, `SameSite=Lax`, and `Secure` unless you turn
that off for a plain-HTTP install. `SameSite=Lax` plus a JSON body on every
mutating endpoint is the CSRF answer: a cross-site form cannot send
`application/json`, and cannot carry the cookie either.

Failed logins back off exponentially, keyed on client address **and** username
together. Keyed on the address alone, one attacked account would lock out every
login from that address; keyed on the username alone, anyone could lock the
administrator out from anywhere.

A wrong username and a wrong password take the same path and take the same
time. The password is verified even when the username does not match, because
returning early on the username was a timing oracle for the administrator's
name — and that name is half of the limiter key.

## Behind a reverse proxy

`SHIMAU_TRUSTED_PROXY_HEADER` names the header carrying the real client
address: `X-Forwarded-For` for nginx, Traefik and Caddy, `CF-Connecting-IP` for
Cloudflare.

Set it **only when shimau cannot be reached except through that proxy**, and
understand both failure modes:

- **Unset, behind a tunnel**, every request arrives from the tunnel's own
  address. The per-address half of the limiter key collapses, so six wrong
  guesses at `admin` from a stranger hold you out too, for as long as they keep
  guessing.
- **Set, on an instance also reachable directly**, a caller writes the header
  itself and picks its own limiter key, which is the same as having no limiter.

Putting Cloudflare Access, Tailscale or a VPN in front is a good idea. It is a
layer on top of shimau's own authentication, never a replacement for it.

## API tokens

A client that is not a browser presents `Authorization: Bearer shimau_…`
instead of the cookie. Tokens are 256 bits from the OS CSPRNG, stored as a
SHA-256 and never in the clear, shown once at creation, and revocable one at a
time.

| Capability | May |
| --- | --- |
| `read` | Every read: stacks, status, logs, resource usage, Compose content, operations |
| `operate` | The same, plus start, stop, restart, update |

**No capability writes a file, and no token can reach the token endpoints.**
Both limits are deliberate. A machine that can save a Compose file and then
start the stack can give a service `privileged: true` and a bind mount of `/`,
which is root on the host in two calls — `docker compose config` accepts every
line of that, because it validates syntax, not intent. And a token that could
mint a token would be its own way out of its capability.

> **A token reads your secrets.** Compose files carry them inline in
> `environment:`, and applications print connection strings into their logs at
> startup. The `.env` masking in the browser is a display choice and changes
> none of that. Treat a token like the administrator password. The list shows
> when each was last used, which is what makes the unused ones safe to revoke.

## The Docker socket

**It is a privilege boundary you cannot mount away.** A read-only bind of
`/var/run/docker.sock` is not a security control: the Docker API grants
containers, and containers grant the host.

shimau's answer is not to narrow the socket but to narrow itself. The set of
commands it can build is closed — `up -d`, `stop`, `restart`, `pull`, `ps`,
`logs`, `stats`, `config` — and lives in one file, `backend/src/compose/mod.rs`.
There is no endpoint that takes a command, a subcommand, an image or a flag
from the client. A new capability is a new enum variant with its own handler,
not a new string.

## Paths

**Nothing from a request becomes a path.** The only client-supplied identifier
that reaches the filesystem is a stack name, and it passes two independent
gates in `backend/src/stacks/paths.rs`: a character allowlist that rejects
`..`, separators and dotfiles, then canonicalisation checked against the
configured root, which is what catches a symlink pointing out of it.

Filenames are never supplied by the client. They come from a fixed set in the
handler.

## Secrets

- `.env` content is never logged, at any level.
- `.env` and its backup are written `0600`.
- Compose child processes get an environment allowlist, not shimau's own —
  otherwise a Compose file could interpolate `${SHIMAU_ADMIN_PASSWORD}` and
  read it back out through `docker compose config`.
- Internal error context is logged server-side and never returned, because it
  can name filesystem paths.
- Every response under `/api` carries `Cache-Control: no-store`, so no browser,
  proxy or back/forward cache keeps a copy.

## Browser hardening

Every response carries a Content-Security-Policy with `script-src 'self'`: no
CDN, no inline script, no `eval`. An admin panel that drives Docker is the last
place to leave a stored `.env` value one `dangerouslySetInnerHTML` away from
running.

`style-src` is the one concession, because Radix positions floating elements
with inline styles and CodeMirror injects its theme at runtime. Styles are a
far smaller prize than scripts.

If you put shimau behind a proxy that injects its own policy, the stricter of
the two wins — and a policy without `'unsafe-inline'` on `style-src` will break
the editors.

## Reporting something

Open an issue. If it is sensitive, say so in the title and leave the details
out; the repository is small enough that this reaches the maintainer.
