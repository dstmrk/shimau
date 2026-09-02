# syntax=docker/dockerfile:1

# shimau ships as one image containing the Rust binary, the built React assets,
# and a pinned Docker CLI + Compose plugin.
#
# Both builder stages run on the BUILD platform and cross-compile, because
# emulating a Rust build under QEMU for arm64 costs tens of minutes.

# =============================================================================
# Stage 1: frontend — architecture-independent static assets
# =============================================================================
FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS frontend
WORKDIR /build

COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm \
    npm ci --prefer-offline --no-audit --no-fund

COPY frontend/ ./
RUN npm run build

# =============================================================================
# Stage 2: backend — cross-compiled Rust binary
# =============================================================================
FROM --platform=$BUILDPLATFORM rust:1.98-bookworm AS backend
WORKDIR /build

ARG TARGETARCH

# rusqlite compiles SQLite from source, so the cross toolchain has to include a
# C compiler for the target, not just a linker. `.cargo/config.toml` is written
# rather than passed as env vars so `cargo test` in this stage would use the
# same setup.
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-gnu; packages="" ;; \
      arm64) target=aarch64-unknown-linux-gnu; packages="gcc-aarch64-linux-gnu libc6-dev-arm64-cross" ;; \
      *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    if [ -n "$packages" ]; then \
      apt-get update && apt-get install -y --no-install-recommends $packages && rm -rf /var/lib/apt/lists/*; \
    fi; \
    rustup target add "$target"; \
    echo "$target" > /build/.rust-target; \
    mkdir -p /build/.cargo; \
    { \
      echo '[target.aarch64-unknown-linux-gnu]'; \
      echo 'linker = "aarch64-linux-gnu-gcc"'; \
    } > /build/.cargo/config.toml
ENV CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
ENV CARGO_HOME=/usr/local/cargo

COPY backend/Cargo.toml backend/Cargo.lock ./
# Build the dependency graph against a stub so a source-only change does not
# recompile every crate.
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && cargo build --release --locked --target "$(cat /build/.rust-target)" \
    && rm -rf src

COPY backend/src ./src
# `touch` defeats cargo's mtime cache, which the stub above just poisoned.
RUN touch src/main.rs src/lib.rs \
    && cargo build --release --locked --target "$(cat /build/.rust-target)" \
    && cp "target/$(cat /build/.rust-target)/release/shimau" /build/shimau

# =============================================================================
# Stage 3: runtime
# =============================================================================
FROM debian:bookworm-slim AS runtime

# Pinned deliberately: an image that manages Compose projects should not
# inherit whatever CLI version happens to be current on rebuild. Bumping these
# is a reviewed change (spec §9.2).
ARG DOCKER_CLI_VERSION=5:29.7.2-1~debian.12~bookworm
ARG DOCKER_COMPOSE_VERSION=5.5.0-1~debian.12~bookworm

RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl gnupg; \
    install -m 0755 -d /etc/apt/keyrings; \
    curl -fsSL https://download.docker.com/linux/debian/gpg \
      | gpg --dearmor -o /etc/apt/keyrings/docker.gpg; \
    chmod a+r /etc/apt/keyrings/docker.gpg; \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian bookworm stable" \
      > /etc/apt/sources.list.d/docker.list; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
      "docker-ce-cli=${DOCKER_CLI_VERSION}" \
      "docker-compose-plugin=${DOCKER_COMPOSE_VERSION}"; \
    apt-get purge -y --auto-remove gnupg; \
    rm -rf /var/lib/apt/lists/*; \
    docker compose version

# The container talks to the host daemon over the mounted socket. It never runs
# a daemon of its own (spec §9.2).
WORKDIR /app

COPY --from=backend /build/shimau /usr/local/bin/shimau
COPY --from=frontend /build/dist /app/static

ENV SHIMAU_DATA_DIR=/app/data
ENV SHIMAU_STATIC_DIR=/app/static
ENV SHIMAU_BIND=0.0.0.0:8080

# Commit the image was built from, surfaced in logs for support.
ARG BUILD_SHA
ENV SHIMAU_BUILD_SHA=$BUILD_SHA

EXPOSE 8080

# shimau runs as root because it needs the host Docker socket, whose group id
# differs per host. Root inside a container with the Docker socket is already
# root-equivalent on the host, so dropping to a nobody user would buy nothing
# while breaking the socket permissions on most installs (spec §7.2).

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1

ENTRYPOINT ["/usr/local/bin/shimau"]
