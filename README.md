<img src="docs/media/logo.png" alt="" width="60">

# shimau

**A tiny, modern Docker Compose manager.**

[![CI](https://github.com/dstmrk/shimau/actions/workflows/ci.yml/badge.svg)](https://github.com/dstmrk/shimau/actions/workflows/ci.yml)
[![Image](https://img.shields.io/badge/ghcr.io-dstmrk%2Fshimau-0069a8)](https://github.com/dstmrk/shimau/pkgs/container/shimau)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

shimau gives you a web UI for the Compose projects already sitting on your
server. Start, stop, restart and update them, follow their logs, edit their
`compose.yaml` and `.env`, and nothing else. Your files stay where they are and
stay in charge: shimau reads the directory, shells out to `docker compose`, and
keeps no copy of anything.

![The shimau dashboard: four stacks, each with its status and its actions](docs/media/dashboard.png)

## What it does

- **Finds your stacks.** Every directory under the configured path holding
  exactly one of `compose.yaml`, `compose.yml`, `docker-compose.yaml` or
  `docker-compose.yml`. The directory name is the stack name.
- **Start · Stop · Restart · Update.** Update is `docker compose pull` then
  `docker compose up -d`, nothing cleverer. Stop is `docker compose stop`, so
  containers, networks and volumes survive.
- **Live output.** Operations and logs stream line by line, with the exact
  command at the top so you can reproduce it by hand.
- **Editors.** The Compose file is edited in place, under its own name, and
  saved only if `docker compose config` accepts it; the previous version stays
  as `<name>.bak`. `.env` opens masked and read-only until you reveal it.

| Compose editor | `.env` editor |
| --- | --- |
| ![The Compose editor, with YAML highlighting and the filename preserved](docs/media/compose-editor.png) | ![The .env editor with every value masked](docs/media/env-editor.png) |

## What it will not do

No standalone containers, no Swarm, no Kubernetes, no remote agents, no web
terminal, no `docker compose down`, no image or volume or network management,
no scheduled updates, no metrics, no multiple users. There is no endpoint that
runs a command you hand it.

That list is the product, not a roadmap. If you want a Docker administration
platform, use one.

## Running it

shimau needs a directory of its own, **outside** the one it manages. Put it
inside and shimau discovers itself, and Stop turns off the panel you pressed it
from.

```bash
mkdir -p ~/shimau && cd ~/shimau
curl -fsSL -O https://raw.githubusercontent.com/dstmrk/shimau/main/compose.yaml
curl -fsSL -o .env https://raw.githubusercontent.com/dstmrk/shimau/main/.env.example
```

Two lines in `.env` are required: where your stacks live, and a password for
the first boot.

```bash
SHIMAU_STACKS_DIR=/home/you/docker-apps
SHIMAU_ADMIN_PASSWORD=$(openssl rand -base64 24)
```

`compose.yaml` reads everything from `.env`, so you never have to edit it.

```bash
docker compose up -d
docker compose logs -f shimau
```

Look for `docker compose available` in the startup log: every action in the UI
depends on it. Then open <http://localhost:8080> and sign in.
`SHIMAU_ADMIN_PASSWORD` is read only on the first boot, and the line can go
afterwards.

> **If the login bounces straight back to the form** with no error, you are on
> plain `http://` and the browser is dropping the `Secure` session cookie. Set
> `SHIMAU_COOKIE_SECURE=false`, or put shimau behind TLS, which is the better
> answer for something that controls Docker.

Images are published for `linux/amd64` and `linux/arm64`, one set per release.
`latest` is the newest tagged version; `X.Y.Z` and `X.Y` pin you to one. Merges
to `main` are tested but never published, so `latest` does not move under you.

### Updating

```bash
cd ~/shimau && docker compose pull && docker compose up -d
```

Your stacks are untouched, because shimau keeps nothing about them. Its own
database, in `./data`, holds one account, its sessions and any API tokens.

### The one requirement that bites

**The stacks path must be identical inside and outside the container.** Your
Compose files use relative bind mounts (`./data:/data`) that the *host* daemon
resolves; mount them elsewhere inside the container and every one of those
volumes points at nothing. So `compose.yaml` writes the path once, uses it
twice:

```yaml
environment:
  SHIMAU_STACKS_DIR: ${SHIMAU_STACKS_DIR:?}
volumes:
  - ${SHIMAU_STACKS_DIR:?}:${SHIMAU_STACKS_DIR:?}   # same on both sides
```

### Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `SHIMAU_STACKS_DIR` | — | **Required.** The directory holding your stacks |
| `SHIMAU_ADMIN_USERNAME` | `admin` | Administrator name, first boot only |
| `SHIMAU_ADMIN_PASSWORD` | — | Bootstrap password, first boot only. At least 12 characters |
| `SHIMAU_DATA_DIR` | `/app/data` | Where `shimau.db` lives |
| `SHIMAU_BIND` | `0.0.0.0:8080` | Listen address |
| `SHIMAU_COOKIE_SECURE` | `true` | Set to `false` only when reaching shimau over plain HTTP |
| `SHIMAU_SESSION_TTL_HOURS` | `168` | Session lifetime |
| `SHIMAU_LOG_TAIL` | `200` | Log lines fetched before following |
| `SHIMAU_TRUSTED_PROXY_HEADER` | — | Header carrying the real client address. See below |
| `SHIMAU_LOG` | `info` | `tracing` filter |

Set `SHIMAU_TRUSTED_PROXY_HEADER` to what your proxy sends —
`X-Forwarded-For` for nginx, Traefik and Caddy, `CF-Connecting-IP` for
Cloudflare — and only when shimau cannot be reached except through it.
Without it, every request behind a tunnel arrives from the tunnel's address,
so the login limiter counts them all together and six wrong guesses at `admin`
from anyone hold you out too. With it set on a shimau that is also reachable
directly, a caller can write the header and pick its own limiter key.

## From a script or an agent

Anything that is not a browser authenticates with an API token instead of the
session cookie. Create one from the key icon in the header. It is shown once.

```bash
curl -H "Authorization: Bearer shimau_…" http://localhost:8080/api/stacks
```

A token carries one of two capabilities. **Read** covers stacks, status, logs,
resource usage, Compose files and operations. **Operate** adds start, stop,
restart and update. Neither can edit a `compose.yaml` or a `.env`: a machine
that can write a Compose file and then start the stack can give a service
`privileged: true` and a bind mount of `/`, and `docker compose config` would
accept every line of it. Editing stays with the browser session.

`docs/openapi.yaml` describes the whole surface. Point a generic OpenAPI-to-MCP
bridge at it if you want an MCP server; shimau does not ship one, because the
capabilities are already the HTTP API.

> **A token reads your secrets.** Compose files carry them inline in
> `environment:`, and applications print connection strings into their logs.
> The `.env` masking in the UI does not change that. Treat a token like the
> administrator password, and revoke the ones you stop using — the list shows
> when each was last seen.

## Security

shimau controls Docker, which makes it an administrative application. Treat it
as one.

- **Authentication is mandatory** and cannot be turned off. One local account,
  Argon2id, a session cookie that is `HttpOnly`, `SameSite=Lax` and `Secure` by
  default, and exponential backoff per address and username.
- **API tokens are stored as a SHA-256**, never in the clear, and are revocable
  one at a time. No token can write a file or mint another token, so a token
  is never a way out of its own capability.
- **The Docker socket is a privilege boundary you cannot mount away.** A
  read-only bind of `/var/run/docker.sock` is not a control. The answer is a
  small closed set of operations, not a Docker API proxy.
- **Nothing from the browser becomes a path.** A stack name goes through a
  character allowlist, then a canonical-path check against the configured
  directory. A symlink pointing out of it is refused.
- **Compose subprocesses get an environment allowlist**, not shimau's own, so a
  Compose file cannot interpolate `${SHIMAU_ADMIN_PASSWORD}` and read it back.
- **`.env` content is never logged**, `.env` and its backup are `0600`, and
  every API response carries `Cache-Control: no-store`.
- **Every response carries a Content-Security-Policy** with `script-src 'self'`:
  no CDN, no inline script, no `eval`. Behind a proxy that injects its own, the
  stricter of the two wins, and one without `'unsafe-inline'` on `style-src`
  breaks the editors.

Cloudflare Access, Tailscale or a VPN in front is a good idea, as a layer on
top of shimau's authentication rather than a replacement for it.

## Development

Rust 1.85+ and Node 22+.

```bash
# API on :8080
cd backend
SHIMAU_STACKS_DIR=/path/to/your/stacks \
SHIMAU_DATA_DIR=./data \
SHIMAU_STATIC_DIR=../frontend/dist \
SHIMAU_COOKIE_SECURE=false \
SHIMAU_ADMIN_PASSWORD=dev-password-please \
cargo run

# UI on :5173, proxying /api to :8080
cd frontend && npm install && npm run dev
```

```bash
cd backend && cargo test          # unit + HTTP suite
cd frontend && npm run test       # Vitest
node scripts/check-docs.mjs       # documentation references
```

The Compose validation tests shell out to `docker compose config`, which needs
the CLI but not a running daemon. Without the CLI they skip themselves.

`docs/spec.md` is the specification the project is built from. `CLAUDE.md` and
`docs/architecture/INDEX.md` are the map for whoever, or whatever, works on it
next.

## Why it exists

The name is しまう, Japanese for putting something away, stowing it where it
belongs. That is the job: your Compose projects, tidy and reachable, owned by
the filesystem rather than by shimau.

Dockge had the right core idea, a file-based Compose UI that does not try to
replace the whole Docker administration ecosystem. shimau keeps it, cuts
further, and rebuilds on Rust and React.

## Licence

MIT. See `LICENSE`.
