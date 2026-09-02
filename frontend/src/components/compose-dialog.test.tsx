import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { render, screen, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { ComposeDialog } from "@/components/compose-dialog"
import { ThemeProvider } from "@/components/theme-provider"

const FILE = {
  filename: "compose.yaml",
  content: "services:\n  web:\n    image: nginx\n",
}

const client = new QueryClient()

function jsonResponse(status: number, body: unknown) {
  return Promise.resolve(
    new Response(JSON.stringify(body), {
      status,
      headers: { "Content-Type": "application/json" },
    })
  )
}

/**
 * Mounts the dialog the way the dashboard does: with an inline
 * `onOpenChange`, so every render of the parent hands it a new closure.
 * `tick` exists only to force those re-renders — a button would sit behind
 * the dialog's backdrop and be unreachable by role.
 */
function Harness({ stack, tick }: { stack: string | null; tick: number }) {
  void tick
  return (
    <ThemeProvider>
      <QueryClientProvider client={client}>
        <ComposeDialog stack={stack} onOpenChange={(open) => void open} />
      </QueryClientProvider>
    </ThemeProvider>
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe("ComposeDialog", () => {
  it("loads the file once and survives a parent re-render", async () => {
    const fetchMock = vi.fn<typeof fetch>(() => jsonResponse(200, FILE))
    vi.stubGlobal("fetch", fetchMock)

    const { rerender } = render(<Harness stack="octotracker" tick={0} />)
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))

    // The dashboard re-renders on every ten-second stack poll, handing the
    // dialog a fresh onOpenChange. That must not refetch: a refetch would
    // overwrite whatever the user had typed and not yet saved.
    rerender(<Harness stack="octotracker" tick={1} />)
    rerender(<Harness stack="octotracker" tick={2} />)
    await Promise.resolve()

    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it("shows the filename it is editing", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>(() =>
        jsonResponse(200, { ...FILE, filename: "docker-compose.yml" })
      )
    )

    render(<Harness stack="grafana" tick={0} />)
    await waitFor(() =>
      expect(
        screen.getByText(/grafana — docker-compose\.yml/)
      ).toBeInTheDocument()
    )
  })

  // Typing into the editor is not tested here: CodeMirror needs layout APIs
  // jsdom does not implement. The save path that matters is covered where it
  // actually lives — backend/tests/api.rs asserts a rejected candidate cannot
  // overwrite the file, and api.test.ts asserts the validator output survives
  // into ApiError.details.
})
