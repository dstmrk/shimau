import { afterEach, describe, expect, it, vi } from "vitest"

import { ApiError, api } from "@/lib/api"

function respond(status: number, body: unknown) {
  return Promise.resolve(
    new Response(body === undefined ? null : JSON.stringify(body), {
      status,
      headers: { "Content-Type": "application/json" },
    })
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe("api client", () => {
  it("returns the parsed body on success", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => respond(200, [{ name: "octotracker" }]))
    )
    await expect(api.listStacks()).resolves.toEqual([{ name: "octotracker" }])
  })

  it("turns an error body into an ApiError carrying code and details", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        respond(422, {
          code: "validation_failed",
          message: "compose.yaml was not saved",
          details: "yaml: line 3: did not find expected ','",
        })
      )
    )

    const error = await api
      .writeCompose("octotracker", "broken")
      .catch((caught) => caught)

    expect(error).toBeInstanceOf(ApiError)
    expect(error.status).toBe(422)
    expect(error.code).toBe("validation_failed")
    expect(error.details).toContain("did not find expected")
  })

  it("surfaces the retry delay of a throttled login", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        respond(429, {
          code: "rate_limited",
          message: "too many failed login attempts",
          retry_after_secs: 8,
        })
      )
    )

    const error = await api.login("admin", "wrong").catch((caught) => caught)
    expect(error.code).toBe("rate_limited")
    expect(error.retryAfterSecs).toBe(8)
  })

  it("treats an unreachable server as a network error rather than throwing raw", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.reject(new TypeError("failed to fetch")))
    )
    const error = await api.me().catch((caught) => caught)
    expect(error).toBeInstanceOf(ApiError)
    expect(error.code).toBe("network")
  })

  it("handles the empty 204 body of logout", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve(new Response(null, { status: 204 })))
    )
    await expect(api.logout()).resolves.toBeUndefined()
  })

  it("escapes stack names in the URL", async () => {
    const fetchMock = vi.fn<typeof fetch>(() => respond(200, { lines: [] }))
    vi.stubGlobal("fetch", fetchMock)
    await api.logs("weird name", 10)
    expect(fetchMock.mock.calls[0][0]).toBe(
      "/api/stacks/weird%20name/logs?tail=10"
    )
  })
})
