import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"

import { Dashboard } from "@/components/dashboard"
import { ThemeProvider } from "@/components/theme-provider"
import type { Stack } from "@/lib/types"

function stack(overrides: Partial<Stack> = {}): Stack {
  return {
    name: "octotracker",
    kind: "valid",
    compose_file: "compose.yaml",
    has_env_file: false,
    status: "running",
    ...overrides,
  }
}

function jsonResponse(status: number, body: unknown) {
  return Promise.resolve(
    new Response(JSON.stringify(body), {
      status,
      headers: { "Content-Type": "application/json" },
    })
  )
}

function renderDashboard(stacks: Stack[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn<typeof fetch>(() => jsonResponse(200, stacks))
  )
  const client = new QueryClient()
  render(
    <ThemeProvider>
      <QueryClientProvider client={client}>
        <Dashboard
          identity={{ username: "admin", version: "0.0.0" }}
          onSignedOut={vi.fn()}
        />
      </QueryClientProvider>
    </ThemeProvider>
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe("Dashboard search", () => {
  it("shows every stack until the operator types", async () => {
    renderDashboard([
      stack({ name: "octotracker" }),
      stack({ name: "grafana" }),
    ])
    await waitFor(() =>
      expect(screen.getByText("octotracker")).toBeInTheDocument()
    )
    expect(screen.getByText("grafana")).toBeInTheDocument()
  })

  it("filters by name as the operator types, case-insensitively", async () => {
    renderDashboard([
      stack({ name: "octotracker" }),
      stack({ name: "grafana" }),
    ])
    await waitFor(() =>
      expect(screen.getByText("octotracker")).toBeInTheDocument()
    )

    await userEvent.type(screen.getByRole("searchbox"), "GRAF")

    expect(screen.queryByText("octotracker")).toBeNull()
    expect(screen.getByText("grafana")).toBeInTheDocument()
  })

  it("tells the operator no stack matches, without claiming the directory is empty", async () => {
    renderDashboard([stack({ name: "octotracker" })])
    await waitFor(() =>
      expect(screen.getByText("octotracker")).toBeInTheDocument()
    )

    await userEvent.type(screen.getByRole("searchbox"), "nothing-matches-this")

    expect(screen.queryByText("octotracker")).toBeNull()
    expect(screen.getByText(/No stacks match/)).toBeInTheDocument()
    expect(screen.queryByText(/No stacks found/)).toBeNull()
  })

  it("clearing the search restores the full list", async () => {
    renderDashboard([
      stack({ name: "octotracker" }),
      stack({ name: "grafana" }),
    ])
    await waitFor(() =>
      expect(screen.getByText("octotracker")).toBeInTheDocument()
    )

    const search = screen.getByRole("searchbox")
    await userEvent.type(search, "graf")
    expect(screen.queryByText("octotracker")).toBeNull()

    await userEvent.clear(search)
    expect(screen.getByText("octotracker")).toBeInTheDocument()
    expect(screen.getByText("grafana")).toBeInTheDocument()
  })
})
