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
  `up -d`. Stop is `docker compose stop`, so containers, networks and volumes
  survive.
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
`latest` is the newest tagged version, `X.Y.Z` and `X.Y` pin you to one, and
merges to `main` are tested but never published.

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
volumes points at nothing. `compose.yaml` writes `SHIMAU_STACKS_DIR` once and
uses it for both sides of the mount, which is why you never edit that file.

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
| `SHIMAU_TRUSTED_PROXY_HEADER` | — | Real client address behind a proxy. Read `docs/security.md` before setting it |
| `SHIMAU_LOG` | `info` | `tracing` filter |

## From a script or an agent

Anything that is not a browser authenticates with an API token instead of the
session cookie. Create one from the key icon in the header; it is shown once.

```bash
curl -H "Authorization: Bearer shimau_…" http://localhost:8080/api/stacks
```

A token carries **read** or **operate** — the second adds the four lifecycle
actions. Neither can edit a `compose.yaml` or a `.env`, and a token reads every
secret your Compose files and logs contain.

`docs/openapi.yaml` describes the whole surface. Point a generic OpenAPI-to-MCP
bridge at it if you want an MCP server; shimau does not ship one, because the
capabilities are already the HTTP API.

## Security

shimau controls Docker, which makes it an administrative application. Treat it
as one. Authentication is mandatory and cannot be turned off, the Docker socket
is a boundary you cannot mount away, nothing from a request ever becomes a
filesystem path, and `.env` is never logged.

`docs/security.md` is the whole picture, including what shimau leaves to you.

## More

- `docs/development.md` — running the two halves, the tests, what CI runs.
- `docs/spec.md` — the specification the project is built from.
- `docs/architecture/INDEX.md` — the codebase map, for anyone changing it.

## Why it exists

The name is しまう, Japanese for putting something away, stowing it where it
belongs. That is the job: your Compose projects, tidy and reachable, owned by
the filesystem rather than by shimau.

Dockge had the right core idea, a file-based Compose UI that does not try to
replace the whole Docker administration ecosystem. shimau keeps it and cuts
further.

## Licence

MIT. See `LICENSE`.
