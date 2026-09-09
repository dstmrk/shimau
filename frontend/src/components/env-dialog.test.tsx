import { render, screen, waitFor } from "@testing-library/react"

import { afterEach, describe, expect, it, vi } from "vitest"

import { EnvDialog } from "@/components/env-dialog"
import { ThemeProvider } from "@/components/theme-provider"

function jsonResponse(status: number, body: unknown) {
  return Promise.resolve(
    new Response(JSON.stringify(body), {
      status,
      headers: { "Content-Type": "application/json" },
    })
  )
}

function mount(stack = "grafana") {
  render(
    <ThemeProvider>
      <EnvDialog stack={stack} onOpenChange={() => {}} />
    </ThemeProvider>
  )
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe("EnvDialog", () => {
  it("masks an existing file and keeps it read-only until revealed", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        jsonResponse(200, {
          filename: ".env",
          exists: true,
          content: "TOKEN=secret\n",
        })
      )
    )
    mount("octotracker")

    expect(
      await screen.findByRole("button", { name: /Reveal values to edit/ })
    ).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
    expect(screen.queryByText(/TOKEN=secret/)).toBeNull()
  })

  /// A 404 is "this stack has no .env", which is an offer rather than a
  /// failure: the dialog stays open on an empty, editable buffer.
  it("offers an empty editor when the stack has no .env", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        jsonResponse(404, { code: "not_found", message: "grafana has no .env" })
      )
    )
    mount()

    expect(await screen.findByText(/has no .env yet/)).toBeInTheDocument()
    // Nothing on disk to protect, so no reveal gate stands in the way.
    expect(
      screen.queryByRole("button", { name: /Reveal values to edit/ })
    ).toBeNull()
  })

  // Typing into the editor is not tested here, for the reason the Compose
  // dialog's suite gives: CodeMirror needs layout APIs jsdom does not
  // implement. What the creation path does with the content it is given is
  // asserted where it happens — `an_env_file_can_be_created_where_there_was_none`
  // in backend/tests/api.rs writes the file, checks its `0600` mode, and
  // checks no backup was invented for a file that did not exist.
  it("has nothing to save until something is typed", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        jsonResponse(404, { code: "not_found", message: "grafana has no .env" })
      )
    )
    mount()

    await screen.findByText(/has no .env yet/)
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled()
  })

  it("closes on a failure that is not a missing file", async () => {
    const onOpenChange = vi.fn()
    vi.stubGlobal(
      "fetch",
      vi.fn(() =>
        jsonResponse(500, { code: "internal", message: "internal error" })
      )
    )
    render(
      <ThemeProvider>
        <EnvDialog stack="grafana" onOpenChange={onOpenChange} />
      </ThemeProvider>
    )

    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false)
    })
  })
})
