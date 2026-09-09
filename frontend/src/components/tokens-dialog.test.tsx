import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import { ThemeProvider } from "@/components/theme-provider"
import { TokensDialog } from "@/components/tokens-dialog"

const EXISTING = {
  id: 7,
  label: "laptop",
  capability: "read" as const,
  created_at: 1_700_000_000,
  last_used_at: null,
}

function jsonResponse(status: number, body: unknown) {
  return Promise.resolve(
    new Response(JSON.stringify(body), {
      status,
      headers: { "Content-Type": "application/json" },
    })
  )
}

function mount() {
  render(
    <ThemeProvider>
      <TokensDialog open onOpenChange={() => {}} />
    </ThemeProvider>
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe("TokensDialog", () => {
  it("lists the tokens it is given", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => jsonResponse(200, [EXISTING]))
    )
    mount()

    expect(await screen.findByText("laptop")).toBeInTheDocument()
    expect(screen.getByText(/never used/)).toBeInTheDocument()
  })

  it("says so when there are none", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => jsonResponse(200, []))
    )
    mount()

    expect(await screen.findByText(/No tokens yet/)).toBeInTheDocument()
  })

  /// The token exists in exactly one place after creation: this dialog. If it
  /// is not on screen, it is gone.
  it("shows the new token once, with the warning that it cannot be shown again", async () => {
    const fetchMock = vi.fn(
      (_input: string | URL | Request, init?: RequestInit) => {
        if (init?.method === "POST") {
          return jsonResponse(201, {
            ...EXISTING,
            id: 8,
            label: "ci",
            secret: "shimau_thesecretvalue",
          })
        }
        return jsonResponse(200, [])
      }
    )
    vi.stubGlobal("fetch", fetchMock)
    mount()

    await screen.findByText(/No tokens yet/)
    await userEvent.type(screen.getByLabelText("Label"), "ci")
    await userEvent.click(screen.getByRole("button", { name: /Create token/ }))

    expect(await screen.findByText("shimau_thesecretvalue")).toBeInTheDocument()
    expect(screen.getByText(/cannot show it again/)).toBeInTheDocument()
  })

  it("will not create a token without a label", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => jsonResponse(200, []))
    )
    mount()

    await screen.findByText(/No tokens yet/)
    expect(screen.getByRole("button", { name: /Create token/ })).toBeDisabled()

    await userEvent.type(screen.getByLabelText("Label"), "   ")
    expect(screen.getByRole("button", { name: /Create token/ })).toBeDisabled()
  })

  it("selects a capability and sends it", async () => {
    const fetchMock = vi.fn(
      (_input: string | URL | Request, init?: RequestInit) => {
        if (init?.method === "POST") {
          return jsonResponse(201, { ...EXISTING, secret: "shimau_x" })
        }
        return jsonResponse(200, [])
      }
    )
    vi.stubGlobal("fetch", fetchMock)
    mount()

    await screen.findByText(/No tokens yet/)
    await userEvent.type(screen.getByLabelText("Label"), "ci")

    const operate = screen.getByRole("button", { name: /Operate/ })
    await userEvent.click(operate)
    expect(operate).toHaveAttribute("aria-pressed", "true")
    expect(screen.getByRole("button", { name: /^Read/ })).toHaveAttribute(
      "aria-pressed",
      "false"
    )

    await userEvent.click(screen.getByRole("button", { name: /Create token/ }))
    await waitFor(() => {
      const post = fetchMock.mock.calls.find(
        ([, init]) => (init as RequestInit | undefined)?.method === "POST"
      )
      expect(post).toBeDefined()
      expect(JSON.parse((post![1] as RequestInit).body as string)).toEqual({
        label: "ci",
        capability: "operate",
      })
    })
  })

  it("removes a revoked token from the list", async () => {
    const fetchMock = vi.fn(
      (_input: string | URL | Request, init?: RequestInit) => {
        if (init?.method === "DELETE") {
          return Promise.resolve(new Response(null, { status: 204 }))
        }
        return jsonResponse(200, [EXISTING])
      }
    )
    vi.stubGlobal("fetch", fetchMock)
    mount()

    await screen.findByText("laptop")
    await userEvent.click(screen.getByRole("button", { name: "Revoke laptop" }))

    await waitFor(() => {
      expect(screen.queryByText("laptop")).toBeNull()
    })
  })
})
