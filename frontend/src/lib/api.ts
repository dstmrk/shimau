import type {
  EnvContent,
  FileContent,
  Identity,
  OperationSnapshot,
  OutputLine,
  Stack,
  StackAction,
} from "@/lib/types"

/**
 * An error the API reported. `details` carries the Compose output when there
 * is any, which is the difference between "operation failed" and something
 * the user can act on.
 */
export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly details?: string
  readonly retryAfterSecs?: number

  constructor(
    status: number,
    code: string,
    message: string,
    details?: string,
    retryAfterSecs?: number
  ) {
    super(message)
    this.name = "ApiError"
    this.status = status
    this.code = code
    this.details = details
    this.retryAfterSecs = retryAfterSecs
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response
  try {
    response = await fetch(path, {
      ...init,
      headers: {
        ...(init?.body ? { "Content-Type": "application/json" } : {}),
        ...init?.headers,
      },
    })
  } catch {
    throw new ApiError(0, "network", "shimau is unreachable")
  }

  if (response.status === 204) {
    return undefined as T
  }

  const payload = await response.json().catch(() => null)

  if (!response.ok) {
    throw new ApiError(
      response.status,
      payload?.code ?? "unknown",
      payload?.message ?? response.statusText,
      payload?.details,
      payload?.retry_after_secs
    )
  }

  return payload as T
}

export const api = {
  me: () => request<Identity>("/api/auth/me"),

  login: (username: string, password: string) =>
    request<Identity>("/api/auth/login", {
      method: "POST",
      body: JSON.stringify({ username, password }),
    }),

  logout: () => request<void>("/api/auth/logout", { method: "POST" }),

  listStacks: () => request<Stack[]>("/api/stacks"),

  runAction: (stack: string, action: StackAction) =>
    request<{ operation_id: string }>(
      `/api/stacks/${encodeURIComponent(stack)}/${action}`,
      { method: "POST" }
    ),

  operation: (id: string) =>
    request<OperationSnapshot>(`/api/operations/${encodeURIComponent(id)}`),

  readCompose: (stack: string) =>
    request<FileContent>(`/api/stacks/${encodeURIComponent(stack)}/compose`),

  writeCompose: (stack: string, content: string) =>
    request<FileContent>(`/api/stacks/${encodeURIComponent(stack)}/compose`, {
      method: "PUT",
      body: JSON.stringify({ content }),
    }),

  readEnv: (stack: string) =>
    request<EnvContent>(`/api/stacks/${encodeURIComponent(stack)}/env`),

  writeEnv: (stack: string, content: string) =>
    request<EnvContent>(`/api/stacks/${encodeURIComponent(stack)}/env`, {
      method: "PUT",
      body: JSON.stringify({ content }),
    }),

  logs: (stack: string, tail: number) =>
    request<{ lines: OutputLine[] }>(
      `/api/stacks/${encodeURIComponent(stack)}/logs?tail=${tail}`
    ),
}

/** SSE endpoints. `EventSource` sends the session cookie same-origin. */
export const streams = {
  operation: (id: string) => `/api/operations/${encodeURIComponent(id)}/stream`,
  logs: (stack: string, tail: number) =>
    `/api/stacks/${encodeURIComponent(stack)}/logs/stream?tail=${tail}`,
}
