---
name: release-deploy
description: Use when changing how shimau is built, shipped or self-hosted — the Dockerfile, the CI and publish workflow in .github/workflows/ci.yml, the pinned Docker CLI and Compose plugin versions, multi-architecture builds, the example compose.yaml, or the environment variables an operator sets. Covers why the Rust build cross-compiles while the runtime stage uses QEMU, how images are tagged on GHCR, the identical-path deployment requirement, and why the container runs as root.
---

# release-deploy — one image, two architectures

## What ships

`ghcr.io/dstmrk/shimau`, linux/amd64 and linux/arm64, containing the Rust
binary, the built React assets, and a **pinned** Docker CLI and Compose
plugin. Pinning is deliberate: an image whose job is managing Compose projects
should not silently change Compose version on a rebuild (spec §9.2). Bumping
`DOCKER_CLI_VERSION` / `DOCKER_COMPOSE_VERSION` in the Dockerfile is a
reviewed change.

To find the current package versions:

```bash
curl -s https://download.docker.com/linux/debian/dists/bookworm/stable/binary-amd64/Packages \
  | grep -E '^(Package|Version):' | paste - - \
  | grep -E 'docker-ce-cli|docker-compose-plugin' | tail -4
```

Check `binary-arm64` too — both architectures must have the same version, or
the arm64 build fails on an exact-version pin.

## Why the build is shaped the way it is

The frontend and backend stages run on `$BUILDPLATFORM` and cross-compile.
Emulating a Rust build under QEMU for arm64 costs tens of minutes; cross-
compiling costs a linker and a cross libc.

`rusqlite` compiles SQLite from source, so the arm64 build needs
`gcc-aarch64-linux-gnu` **and** `libc6-dev-arm64-cross` — a linker alone is not
enough. This is why the targets are `-gnu` and the runtime is `debian-slim`
rather than musl and Alpine: a musl cross sysroot for aarch64 is not in the
Debian archive.

The runtime stage does run under QEMU, because `apt-get install` has to execute
target binaries. That is one apt transaction, not a compiler.

The dependency layer is built against a stub `main.rs`/`lib.rs` first, then the
real sources are copied and `touch`ed — cargo's mtime cache would otherwise
consider the stub build current.

## Tags

The publish job runs only on a push (never a pull request: a fork's token
cannot write packages) and only after the checks and the image smoke test have
passed.

| trigger          | tags                                   |
| ---------------- | -------------------------------------- |
| push to `main`   | `latest`, `main-<short-sha>`           |
| tag `vX.Y.Z`     | `X.Y.Z`, `X.Y`                         |

A merge to main publishes `latest`. Cutting a release means pushing a `vX.Y.Z`
tag and bumping `version` in `backend/Cargo.toml`.

## Accepted vulnerabilities

`.trivyignore.yaml` carries the findings the image ships with, each scoped to
the binary it was found in, with a reason and an `expired_at`.

Everything in it today is in a Go binary Docker built: the CLI's Go standard
library, and the Compose plugin's vendored `x/crypto`, `x/mod` and `grpc`.
None can be fixed from this repository — the pinned versions are already the
newest published, and only a Docker rebuild moves them. The Debian layer
itself scans clean.

Three rules for that file:

- **Scope every entry to a path.** A CVE ignored globally is a CVE that stops
  failing the build when it appears in *our* code. `paths:` is what keeps the
  gate meaningful.
- **`expired_at` is the review, and nothing else is.** Dependabot's docker
  ecosystem tracks base images, not the apt version pins in the runtime stage,
  so no bot will notice a Docker release that fixes these. When an entry
  lapses, CI goes red and someone reads it again.
- **Never extend a date without re-reading the finding.** The statement has to
  survive being read by someone who did not write it.

`TRIVY_SHOW_SUPPRESSED` is set on the step, so accepted findings still print
in the job log instead of vanishing.

## The image job is the gate

`.github/workflows/ci.yml` builds amd64, starts the container against a real
Docker socket and a throwaway stacks directory, and asserts: the health probe
answers, `docker compose version` works *inside* the image, `/api/stacks` is
401 without a session, and the demo stack is discovered after login. Then
Trivy scans it. Only then does the multi-arch build run.

If you change the Dockerfile, that job is what tells you whether it works.

## The identical-path requirement

`SHIMAU_STACKS_DIR` must be the same path inside and outside the container.
Compose files use relative bind mounts, and the **host** daemon resolves them
against the host filesystem: mounting `/home/user/docker-apps` at `/stacks`
would make every `./data:/data` in every managed stack point somewhere that
does not exist. This is a hard deployment requirement (spec §6.3), not a
convention, and `compose.yaml` says so at the mount.

shimau's own Compose project must live **outside** the managed stacks
directory, or it discovers and manages itself.

## Why the container runs as root

It needs the host Docker socket, whose group id differs per host. Root inside a
container holding that socket is already root-equivalent on the host, so
dropping to an unprivileged user buys nothing while breaking the socket
permissions on most installs. The mitigation that does work is the one shimau
implements: a small, closed set of operations (spec §7.2).
