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
curl -s https://download.docker.com/linux/debian/dists/trixie/stable/binary-amd64/Packages \
  | grep -E '^(Package|Version):' | paste - - \
  | grep -E 'docker-ce-cli|docker-compose-plugin' | tail -4
```

Check `binary-arm64` too — both architectures must have the same version, or
the arm64 build fails on an exact-version pin. The suite in that URL has to
match the Dockerfile's base, and the package version string carries it
(`…-1~debian.13~trixie`), so a base bump and a version bump are the same edit.

## Two versions that must not drift apart

**The Node major in the Dockerfile and in `.github/workflows/ci.yml` are the
same number.** CI lints, type-checks and tests the frontend on one Node; the
image builds the shipped bundle with another. Let those diverge and the suite
is green on a runtime nobody ships. Dependabot bumps the Dockerfile (docker
ecosystem) without touching the workflow pin, so this pair needs a human on
every Node bump.

**Never cancel a run on main.** `cancel-in-progress` is scoped to
`github.ref != 'refs/heads/main'`. Every push to main shares
`refs/heads/main`, so a blanket cancel lets a quick series of merges kill each
other's run. That is not theoretical: five Dependabot PRs merged a minute
apart cancelled the build in flight. Main no longer publishes, but the `image`
job is the only thing that proves a merged commit builds and starts, so
cancelling it leaves a commit nobody ever built — and the next merge tests its
own tree, not that one. Superseded pull-request pushes are still cancelled,
which is where the saving actually is.

## The base: Debian stable, and all three stages together

The frontend builder, the backend builder and the runtime are all Debian 13
(trixie). Two rules:

- **Track Debian stable, not oldstable.** The MVP shipped on bookworm, which
  was already oldstable by then. Nothing was broken — Trivy scanned the Debian
  layer clean — but a base one release behind gets security updates on
  narrower terms, and the gap only widens. Check `deb.debian.org/debian/dists/stable/Release`
  for the current codename when this comes up again.
- **Bump the three stages together.** The binary links against the builder's
  glibc and runs against the runtime's. Move the builder forward alone and the
  container dies at startup with `GLIBC_2.xx not found`; bookworm shipped
  glibc 2.36, trixie ships 2.41. Moving the runtime forward alone happens to
  work, which is worse — it hides the mistake until the next bump.

Alpine was considered and rejected: `rusqlite` compiles SQLite from C, and a
musl cross sysroot for aarch64 is not in the Debian archive, so it would add a
third-party toolchain to the build. It would not have removed a single Trivy
finding either — those live in Docker's own Go binaries, identical on any base.

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

**Only a version tag publishes.** The job is gated on
`startsWith(github.ref, 'refs/tags/v')`, and still runs after the checks and
the image smoke test have passed.

| trigger           | tags                     |
| ----------------- | ------------------------ |
| pull request      | nothing                  |
| push to `main`    | nothing                  |
| tag `vX.Y.Z`      | `X.Y.Z`, `X.Y`, `latest` |
| tag `vX.Y.Z-rc.N` | `X.Y.Z-rc.N`             |

`latest` comes from `flavor: latest=auto`: the metadata action moves it onto a
semver tag and leaves it where it is for a prerelease. `{{major}}.{{minor}}`
degrades to the full version on a prerelease as well, so `v1.3.0-rc.1`
publishes exactly one tag and never claims to be `1.3`. Cutting a release is a
commit bumping `version` in `backend/Cargo.toml`, then a `vX.Y.Z` tag on it.

Why not on every merge: shimau is pulled by hand into other people's homelabs.
A `latest` that moves on every merge — five Dependabot PRs in a minute, a
refactor half landed — reaches them, and it makes "which version are you
running?" unanswerable. Tag-only publishing also makes a release atomic: a tag
whose build fails has published nothing, so recovery is delete the tag, fix,
tag again.

The price is that **arm64 is only built at release time**, since the `image`
job is amd64 by design. A cross-compile or QEMU apt breakage lands on main
unnoticed and surfaces on the tag. That is the accepted trade: validating
arm64 on every merge means running the expensive half of the publish on every
merge, which is the cost being removed, and a failed tag build ships nothing.
So when a change touches the Dockerfile — a base bump, a new apt package, a
Dependabot docker PR — build arm64 by hand before merging:

```bash
docker buildx build --platform linux/arm64 .
```

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
