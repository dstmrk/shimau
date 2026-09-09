/** Wire types, mirroring the Rust serialisation in `backend/src`. */

export type StackStatus =
  "running" | "partial" | "stopped" | "not_created" | "unknown"

export type StackAction = "start" | "stop" | "restart" | "update"

export type OperationStatus = "running" | "succeeded" | "failed"

export interface OutputLine {
  stream: "stdout" | "stderr"
  text: string
}

/**
 * One container's resource usage, as `docker compose stats` formats it —
 * kept as Docker's own strings (e.g. `"12MiB / 512MiB"`), not reparsed.
 */
export interface ContainerStats {
  Name: string
  CPUPerc: string
  MemUsage: string
  MemPerc: string
  NetIO: string
  BlockIO: string
  PIDs: string
}

/** A stack as returned by `GET /api/stacks`. */
export interface Stack {
  name: string
  /** `valid` carries `compose_file`; `ambiguous` carries `compose_files`. */
  kind: "valid" | "ambiguous"
  compose_file?: string
  compose_files?: string[]
  has_env_file: boolean
  status: StackStatus
  active_operation_id?: string
}

export interface OperationSnapshot {
  id: string
  stack: string
  action: StackAction
  status: OperationStatus
  exit_code: number | null
  started_at: number
  finished_at: number | null
  lines: OutputLine[]
  truncated: boolean
}

export interface FileContent {
  filename: string
  content: string
}

export interface EnvContent extends FileContent {
  exists: boolean
}

export interface Identity {
  /** The administrator's name, or a token's label when a token answered. */
  username: string
  /** Version of the backend serving this session, shown in the header. */
  version: string
  /** Which credential answered. The browser is always `session`. */
  principal: "session" | "token"
  capability?: Capability
}

/**
 * What an API token may do. Neither writes a file: a machine that can save a
 * Compose file and start the stack has root on the host, so editing stays
 * with the browser session.
 */
export type Capability = "read" | "operate"

/** A token as returned by `GET /api/tokens`. Never carries the secret. */
export interface ApiToken {
  id: number
  label: string
  capability: Capability
  created_at: number
  last_used_at: number | null
}

/** The one response that carries a token, from `POST /api/tokens`. */
export interface CreatedToken extends ApiToken {
  secret: string
}
