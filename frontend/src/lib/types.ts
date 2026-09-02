/** Wire types, mirroring the Rust serialisation in `backend/src`. */

export type StackStatus =
  "running" | "partial" | "stopped" | "not_created" | "unknown"

export type StackAction = "start" | "stop" | "restart" | "update"

export type OperationStatus = "running" | "succeeded" | "failed"

export interface OutputLine {
  stream: "stdout" | "stderr"
  text: string
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
  username: string
  /** Version of the backend serving this session, shown in the header. */
  version: string
}
