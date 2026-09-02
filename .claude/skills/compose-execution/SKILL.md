---
name: compose-execution
description: Use when adding or changing anything that shells out to Docker — a new stack action, the argv of an existing one, status parsing, log invocation, Compose file validation, or edits to backend/src/compose/mod.rs and backend/src/compose/status.rs. Covers the closed Action enum and why there is no generic exec endpoint, the environment allowlist that stops a Compose file reading manager secrets back out, the working-directory requirement, why stop is never down, and how docker compose ps output is parsed across CLI versions.
---

# compose-execution — a closed set of commands

shimau drives the Compose CLI instead of reimplementing it. The set of
invocations it can produce is closed and lives in `backend/src/compose/mod.rs`.

## The action enum is the whole API

`Action::steps()` maps each action to its Compose invocations:

```text
start   -> up -d
stop    -> stop
restart -> restart
update  -> pull, then up -d
```

There is no endpoint that takes a command, a subcommand, or an image name from
the client, and adding one is out of scope (spec §6.2, §13). A new action means
a new enum variant with its own steps, not a parameter.

**`stop` is `stop`, never `down`.** Containers, networks and volumes survive a
stop; `down` destroys them. A test asserts that no action expands to a step
containing `down`, precisely so nobody adds it as a convenience.

**Update is `pull` then `up -d`, and stops at the first failure.** If the pull
fails, `run_steps` returns without recreating anything: reporting success after
recreating containers on the *old* image is the failure mode that matters.

## The environment allowlist is load-bearing

Every child process gets `env_clear()` and then only `FORWARDED_ENV`: `PATH`,
`HOME` and the `DOCKER_*` variables. Two reasons, both real:

- Compose interpolates `${VAR}` in a Compose file **from its own
  environment**. Inheriting the manager's environment would let anyone who can
  edit a Compose file write `image: ${SHIMAU_ADMIN_PASSWORD}` and read the
  bootstrap password back out of `docker compose config`.
- A stack's values must come from its own `.env`, not from whatever the
  manager container happens to have set.

If you ever need to forward a new variable, add it to that array with a comment
saying why — never widen it to the whole environment.

## Working directory

Every command runs with the stack directory as its cwd, and the Compose file is
pinned with `--file <name>`. Relative bind mounts (`./data:/data`) resolve
against the cwd, which is why the stacks path must be identical on the host and
inside the container (spec §6.3). Passing an absolute `--file` and letting the
cwd drift would silently relocate every relative volume.

`--ansi never` and `--progress plain` keep the output line-oriented: the SSE
stream and the log viewer both consume lines, and TTY progress redraws would
arrive as control-character soup.

## Parsing `docker compose ps`

`backend/src/compose/status.rs` accepts **both** shapes Compose has shipped: a
JSON array and one object per line. Pinning the parser to one would tie the
image to a CLI version.

`service_statuses` returns `Option<Vec<ServiceStatus>>`, and the distinction
matters: `Some(vec![])` means Compose answered and there are no containers
(`NotCreated`), while `None` means the question could not be answered — an
unreachable daemon, a timeout, unparseable output — which is `Unknown`. A
broken Docker socket reading as "nothing created" is a bug, not a status.

A container that is running but reports `Health: unhealthy` does not count as
up, so a stack whose only service is failing its healthcheck reads `stopped`
rather than `running`.

## Reproducibility

Every operation prints the command it is about to run as its first output
line, in `docker compose …` form. That is the contract from spec §15.4: what
the UI did must be reproducible by hand from the stack directory.
