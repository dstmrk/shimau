# shimau — Product & Technical Specification

> **Status:** v0.1, implemented  
> **Audience:** Maintainers and AI coding agents  
> **Repository:** <https://github.com/dstmrk/shimau>  
> **Project name:** shimau

This is the specification the repository is built from. Section 17 records
how each open decision was resolved during the MVP; anything still open is
marked as such. Changing a settled decision (section 16) is a deliberate act,
not a refactor.

## 1. Vision

Build a small, fast, self-hosted web application for managing **Docker Compose projects** through a modern UI.

The project is intentionally **not** a general-purpose Docker management platform. Its core purpose is to provide a better, simpler alternative to tools such as Dockge for users who primarily manage their applications through `docker compose` files.

The application should feel like a thin, reliable UI layer over the Docker Compose CLI:

- Compose files remain the source of truth.
- The filesystem remains the source of truth for stack configuration.
- Docker Compose remains responsible for container orchestration.
- The application should add as little state and complexity as possible.

## 2. Design Principles

1. **Compose-first** — manage Compose projects, not individual containers.
2. **Filesystem-first** — do not import or own application configuration in a proprietary database.
3. **Minimal surface area** — implement only what is necessary for the core workflow.
4. **Safe by default** — no arbitrary shell or Docker command execution from the UI.
5. **Reproducible** — everything performed through the UI should have an equivalent, understandable CLI operation.
6. **Self-hosted** — the application itself runs as a Docker container and should require minimal infrastructure.
7. **Modern UI, boring backend** — use current frontend tooling while keeping the backend small and predictable.

## 3. Scope

### 3.1 Supported stack model

A **stack** is a directory directly below the configured stacks directory.

Example:

```text
/home/user/docker-apps/
├── octotracker/
│   ├── compose.yaml
│   └── .env
├── grafana/
│   └── docker-compose.yml
└── uptime-kuma/
    └── compose.yml
```

The application discovers stacks by scanning the configured stacks directory.

A stack is considered valid when its directory contains exactly one supported Compose filename.

Supported Compose filenames:

- `compose.yaml`
- `compose.yml`
- `docker-compose.yaml`
- `docker-compose.yml`

If no supported Compose file exists, the directory is ignored.

If multiple supported Compose files exist in the same stack directory, the stack should be reported as invalid and no destructive operation should be performed until the ambiguity is resolved.

### 3.2 Optional `.env`

The application recognizes `.env` only when it exists in the same directory as the Compose file.

Example:

```text
/home/user/docker-apps/octotracker/
├── compose.yaml
└── .env
```

The `.env` file is optional.

The application must not attempt to discover or manage unrelated `.env` files elsewhere on the filesystem.

## 4. Core Features

### 4.1 Stack discovery

The application must:

- scan the configured stacks directory;
- detect valid stack directories;
- identify the Compose filename used by each stack;
- detect whether `.env` exists;
- show the runtime status of each stack;
- support rescanning without restarting the application.

The stack name is the directory name.

No separate stack ID is required for the MVP.

### 4.2 Stack status

The UI should show a concise status for every stack.

Minimum statuses:

- **Running**
- **Stopped**
- **Created / Not running**
- **Error / Unknown**

The backend should derive status from Docker Compose / Docker rather than maintaining an independent stack-state database.

### 4.3 Stack actions

The primary actions are:

#### Start

Execute:

```bash
docker compose up -d
```

#### Stop

Execute:

```bash
docker compose stop
```

Do **not** expose `docker compose down` in the MVP.

Stopping a stack must preserve its containers, networks and volumes.

#### Restart

Execute:

```bash
docker compose restart
```

#### Update

Update must mean:

```bash
docker compose pull
docker compose up -d
```

Do not introduce custom container/image update logic in the MVP.

The Compose file remains the source of truth.

### 4.4 Logs

Logs are a first-class action and should remain easily accessible from the main stack UI.

The application must support:

- viewing recent logs;
- streaming/following logs in real time;
- displaying command progress/output while a Compose operation is running;
- handling stopped stacks gracefully when no new logs are available.

The implementation should preferably use HTTP streaming/SSE for server-to-browser log streams unless a strong technical reason requires WebSockets.

### 4.5 Compose file editor

The complete Compose file must be viewable and editable from the UI.

Requirements:

- preserve the original filename;
- edit the actual file on disk;
- do not transparently convert between `compose.yaml`, `compose.yml`, and `docker-compose.*` names;
- provide syntax highlighting for YAML;
- support search within the file;
- provide explicit Save action.

Before replacing the existing file, the backend must validate the candidate configuration using Docker Compose semantics where practical (for example `docker compose config`).

The save should be atomic:

1. write candidate content to a temporary file;
2. validate it;
3. replace the original only on successful validation;
4. retain the original file if validation fails.

### 4.6 `.env` editor

If `.env` exists, it must be viewable and editable from the UI.

For the MVP, `.env` is treated as a text file.

The application must not attempt to reinterpret or normalize its contents.

Because `.env` may contain secrets, the UI should support a protected/default-hidden representation of secret-looking values where practical, with an explicit action to reveal them.

The server must never log `.env` contents.

## 5. UI/UX

### 5.1 Stack-first dashboard

The main screen should present all discovered stacks as compact cards/rows.

Each stack should expose its essential actions without requiring multiple navigation levels.

Conceptually:

```text
┌──────────────────────────────────────────────┐
│ octotracker                    ● Running     │
│                                              │
│ [Stop] [Update] [Restart] [Logs]             │
│ [Compose] [.env]                             │
└──────────────────────────────────────────────┘
```

When a stack is stopped, the primary lifecycle action should become **Start**.

`Logs` should remain accessible regardless of the current status.

`Compose` is always available for a valid stack.

`.env` is shown only when that file exists.

### 5.2 Operation progress

Long-running operations must expose live progress/output rather than only a spinner.

For example:

```text
Updating octotracker...

Pulling ghcr.io/example/octotracker:latest
Pull complete
Recreating octotracker
Started

✓ Update completed
```

The UI should prevent accidental duplicate execution of the same action while an operation is in progress.

### 5.3 Modern component system

Frontend requirements:

- React
- TypeScript
- Vite
- Tailwind CSS
- shadcn/ui components

The application should use shadcn/ui as the component source rather than introducing a second custom component library.

Keep custom styling minimal and purposeful.

Avoid visual complexity, gradients, mascots, and unnecessary dashboard widgets.

## 6. Backend

### 6.1 Technology

Preferred stack:

- Rust
- Axum
- Tokio
- Serde
- `tracing`

The backend should remain small and explicit.

Performance is not the primary reason for Rust; the main benefits are a compact single binary, strong type safety, predictable deployment and a low runtime footprint.

### 6.2 Compose execution model

The backend should invoke the Docker CLI / Compose plugin rather than reimplement Docker Compose behavior.

Expected operations are narrowly defined:

```text
start      -> docker compose up -d
stop       -> docker compose stop
restart    -> docker compose restart
pull       -> docker compose pull
update     -> docker compose pull + docker compose up -d
logs       -> docker compose logs ...
status     -> docker compose ps / docker compose ls as appropriate
validate   -> docker compose config
```

There must be **no generic command execution endpoint**.

Do not implement an API such as:

```http
POST /api/exec
{ "command": "..." }
```

The backend should expose typed operations only.

### 6.3 Working directory

Every Compose command must execute with the stack directory as its working directory.

This is essential because Compose files frequently use relative paths such as:

```yaml
volumes:
  - ./data:/data
```

The stack path must therefore be identical inside and outside the manager container.

Example:

```text
Host:      /home/user/docker-apps
Container: /home/user/docker-apps
```

The project must treat this as a hard deployment requirement.

## 7. Security

This application controls Docker and must be treated as a privileged administrative application.

### 7.1 Authentication

MVP requirements:

- single local administrator account;
- authentication always enabled;
- password stored only as a strong password hash;
- Argon2id preferred for password hashing;
- authenticated session via secure, HttpOnly cookie;
- SameSite policy appropriate for a local administrative application;
- logout support;
- rate limiting / backoff for repeated failed logins.

Do not implement multi-user RBAC in the MVP.

External identity providers, Cloudflare Access, Tailscale and similar systems may be used as an additional network/security layer, but they do not replace the application's own authentication in the MVP.

### 7.2 Docker socket

The manager requires access to the host Docker daemon, normally through:

```text
/var/run/docker.sock
```

The Docker socket must be mounted into the container.

A read-only bind mount of the socket must **not** be treated as a security boundary; Docker API capabilities remain privileged.

The application must instead minimize the operations it exposes.

### 7.3 Path security

All stack paths originate from the configured stacks directory.

The backend must reject path traversal and must ensure resolved paths remain inside the configured stacks directory.

The browser must never be able to submit an arbitrary filesystem path as the target of a file operation.

For example, requests equivalent to:

```text
../../etc/passwd
```

must be rejected.

### 7.4 Secrets

The application must avoid:

- logging `.env` contents;
- returning secrets unnecessarily;
- putting secrets into operation logs;
- persisting plaintext passwords.

### 7.5 No web terminal

The MVP must not include a web terminal or arbitrary container shell.

This is an intentional security and scope decision.

## 8. Persistence

The application should not maintain application/stack state in a database.

The filesystem and Docker Compose are authoritative.

A small SQLite database is acceptable for **manager-owned metadata**, primarily authentication and future UI/application settings.

The database filename should use the eventual product name:

```text
shimau.db
```

Layout:

```text
/app/data/shimau.db
```

The database must never become the source of truth for:

- Compose files;
- `.env` files;
- container status;
- images;
- volumes;
- networks;
- stack definitions.

For the initial implementation, database usage should be kept as small as possible.

## 9. Self-Hosting and Deployment

The application must be fully self-hostable as a Docker container.

The recommended structure is:

```text
/home/user/
├── shimau/
│   ├── compose.yaml
│   └── data/
│
└── docker-apps/
    ├── octotracker/
    │   ├── compose.yaml
    │   └── .env
    └── ...
```

The manager's own Compose project must live outside the configured stacks directory so that it does not manage itself accidentally.

### 9.1 Required mounts

Conceptually:

```yaml
volumes:
  - /var/run/docker.sock:/var/run/docker.sock
  - /home/user/docker-apps:/home/user/docker-apps
  - ./data:/app/data
```

The exact container paths may vary for application data, but the stacks path must be identical inside and outside the container.

### 9.2 Runtime image

Use a multi-stage build.

The runtime image should contain:

- the compiled Rust binary;
- the built React static assets;
- Docker CLI;
- Docker Compose v2 plugin.

The runtime container must **not** run a Docker daemon.

The host Docker daemon remains responsible for containers.

The Docker CLI and Compose plugin versions should be pinned/reviewed as part of application releases rather than relying on whatever Docker CLI happens to exist on the host.

## 10. API Design

The API should be intentionally small.

Illustrative endpoints:

```text
POST /api/auth/login
POST /api/auth/logout
GET  /api/auth/me

GET  /api/stacks
GET  /api/stacks/:stack

POST /api/stacks/:stack/start
POST /api/stacks/:stack/stop
POST /api/stacks/:stack/restart
POST /api/stacks/:stack/update

GET  /api/stacks/:stack/logs
GET  /api/stacks/:stack/logs/stream

GET  /api/stacks/:stack/compose
PUT  /api/stacks/:stack/compose

GET  /api/stacks/:stack/env
PUT  /api/stacks/:stack/env
```

Exact naming can change during implementation, but the API must remain operation-specific and explicit.

The API must not expose arbitrary command execution.

## 11. Error Handling

Errors must be actionable and tied to the underlying Compose operation where possible.

Bad:

```text
Operation failed.
```

Better:

```text
Update failed.

docker compose pull returned exit code 1.

See operation output for details.
```

The UI should preserve useful command output while avoiding secret leakage.

A failed operation must not leave the frontend permanently stuck in a loading state.

## 12. File Safety

File editing is privileged functionality and must be defensive.

### Compose save

The implementation must:

- validate the candidate content;
- use an atomic replacement strategy;
- preserve the current file if validation fails;
- avoid following paths outside the stack directory.

A lightweight backup of the previous file is recommended for the MVP, e.g.:

```text
compose.yaml.bak
```

The backup mechanism must not grow into a full version-control system.

### `.env` save

The same path restrictions and atomic-write principles apply.

Backups for `.env` should be treated carefully because they contain secrets and must inherit appropriate filesystem permissions.

## 13. Non-Goals

The following are explicitly out of scope for the MVP:

- standalone container management;
- Docker Swarm;
- Kubernetes;
- multi-host / remote agents;
- web terminal;
- arbitrary container exec;
- Docker network management;
- Docker volume management;
- image management independent of Compose;
- registry management;
- GitOps / Git integration;
- automatic scheduled updates;
- notifications;
- metrics and resource graphs;
- user management / RBAC;
- OAuth/OIDC providers;
- conversion from `docker run` to Compose;
- Compose `down` from the UI;
- automatic modification of Compose structure beyond explicit file editing.

Additional features may be added later, but only through an explicit scope decision.

## 14. Functional Acceptance Criteria

The MVP is considered functional when all of the following are true:

1. A user can start the application using Docker Compose.
2. The application can scan a configured stacks directory.
3. Valid stacks using any of the four supported Compose filenames are detected.
4. The correct Compose filename is preserved and edited in place.
5. An adjacent `.env` file is detected, displayed and editable.
6. The UI shows the runtime state of each stack.
7. Start, Stop and Restart work through Docker Compose.
8. Update performs `docker compose pull` followed by `docker compose up -d`.
9. Logs can be viewed and followed in real time.
10. Long-running operations expose live output/progress.
11. Invalid Compose content cannot overwrite the existing Compose file.
12. Stack file operations cannot escape the configured stacks directory.
13. The application requires authentication.
14. Passwords are never stored in plaintext.
15. There is no arbitrary command execution endpoint.
16. The manager itself can run as an ordinary Docker Compose project outside the managed stacks directory.
17. The managed stacks path is identical inside and outside the manager container.
18. A clean reinstall/redeployment of the manager does not require migrating the managed Compose files.

## 15. Development Guidelines for AI Coding Agents

This repository is expected to be developed significantly with AI coding agents. Agents must optimize for correctness, reviewability and constrained scope.

### 15.1 Scope discipline

Before implementing a feature, verify that it exists in this specification.

Do not add “helpful” platform features such as:

- arbitrary Docker commands;
- extra Docker resource managers;
- complex configuration databases;
- multi-host infrastructure;
- terminal access;
- background schedulers.

### 15.2 Security first

Any code touching:

- filesystem paths;
- Docker socket;
- command execution;
- authentication;
- cookies/sessions;
- secrets;

must receive dedicated security review.

### 15.3 Testing expectations

Tests should cover at minimum:

- stack discovery;
- all four Compose filenames;
- duplicate Compose filenames;
- missing `.env`;
- path traversal rejection;
- authentication failures and rate limiting;
- permission checks;
- Compose validation failures;
- atomic file saves;
- action-to-command mapping;
- operation failures and non-zero exit codes;
- log streaming lifecycle.

Integration tests should use disposable test Compose projects rather than the developer's real Docker stacks.

### 15.4 Reproducibility

Every Compose command used by the backend should be reproducible manually from the stack directory.

When debugging a failure, the application should make it possible to understand the equivalent CLI command without exposing internal implementation details or secrets.

## 16. Decisions Already Made

The following decisions are considered settled unless deliberately revisited:

- Compose-only management.
- One directory = one stack.
- Four supported Compose filenames.
- Optional `.env` in the same directory.
- Start / Stop / Restart / Update / Logs.
- No `docker compose down` in the MVP.
- Compose and `.env` are edited directly on disk.
- No stack state database.
- Small SQLite database only for manager-owned metadata/auth, named `<tool-name>.db`.
- Authentication is mandatory.
- Single local administrator for MVP.
- Rust + Axum backend.
- React + TypeScript + Vite frontend.
- Tailwind CSS + shadcn/ui.
- Docker Compose CLI is used instead of reimplementing Compose behavior.
- No arbitrary command execution.
- No web terminal.
- Manager is self-hosted through Docker Compose.
- Managed stack path must be identical on host and inside the manager container.
- Manager's own Compose project lives outside the managed stacks directory.

## 17. Open Decisions — as resolved in the MVP

These were left open until implementation planning. Each is recorded with the
decision taken; the two still open say so.

| Question | Resolution |
| --- | --- |
| Final product/repository name | `shimau` |
| Visual design/theme | shadcn/ui preset `b5KJfbfow`: style `base-mira`, base colour `mist`, accent `sky`, Geist, small radius, Lucide icons |
| Docker CLI / Compose versions | Pinned as build arguments in the `Dockerfile` and bumped as a reviewed change |
| SQLite schema | Two tables — `users` (with `CHECK (id = 1)`) and `sessions`; created idempotently, no migration history yet |
| Session storage | Server-side rows in `sessions`, keyed on the SHA-256 of the token; the cookie carries the token, the database never sees it |
| SSE/event model | Three named events — `line`, `finished`, `lagged`; buffered output replayed on subscribe, then live |
| Backup retention | Exactly one previous version, `<name>.bak`, overwritten on each save. Deliberately not a version-control system |
| `.env` masking | Purely a UI reveal. The server returns the file verbatim; the editor masks values and stays read-only until revealed, so a masked buffer can never be saved back |
| API naming and response schemas | As implemented in section 10, with an error body of `{ code, message, details?, retry_after_secs? }` |

Still open, deliberately:

- Whether a `.env` can be **created** from the UI for a stack that has none.
  Today the endpoint answers 404, following section 4.6.
- Whether the client address should be read from a proxy header when shimau
  runs behind a reverse proxy. Today it is the socket address, because an
  unauthenticated header would turn the login limiter off.

These must not expand the product scope without an explicit review.

## 18. Reference: Why the Project Exists

The project is inspired by the useful core of Dockge: a file-based, Compose-focused management UI that intentionally does not try to replace the entire Docker administration ecosystem.

The new project keeps that core idea while deliberately reducing the feature set and attack surface, and replacing the UI/backend implementation with a modern React/Rust stack.

The goal is not to reproduce Dockge feature-for-feature. The goal is to provide a small, maintainable and security-conscious Compose manager for straightforward self-hosted deployments.
