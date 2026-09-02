import { render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { ThemeProvider, useTheme } from "@/components/theme-provider"

function Probe() {
  const { theme } = useTheme()
  return <span data-testid="theme">{theme}</span>
}

/**
 * Replaces `window.localStorage` for one test. `configurable: true` so the
 * afterEach below can put the real one back.
 */
function stubLocalStorage(value: unknown) {
  Object.defineProperty(window, "localStorage", {
    value,
    configurable: true,
    writable: true,
  })
}

const realLocalStorage = window.localStorage

afterEach(() => {
  stubLocalStorage(realLocalStorage)
  vi.unstubAllGlobals()
})

describe("ThemeProvider", () => {
  it("falls back to the default theme when storage is empty", () => {
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>
    )
    expect(screen.getByTestId("theme")).toHaveTextContent("system")
  })

  // Node 26's test environment reaches the provider with no localStorage at
  // all, which took CI red the moment the workflow moved off Node 22. The same
  // crash is reachable in a real browser, so the guard belongs in the provider
  // rather than in the test setup.
  it("renders when localStorage does not exist", () => {
    stubLocalStorage(undefined)
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>
    )
    expect(screen.getByTestId("theme")).toHaveTextContent("system")
  })

  // A browser set to block site data throws on access instead of returning
  // null — the failure mode that actually reaches users.
  it("renders when reading localStorage throws", () => {
    stubLocalStorage({
      getItem() {
        throw new DOMException("denied", "SecurityError")
      },
      setItem() {
        throw new DOMException("denied", "SecurityError")
      },
    })
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>
    )
    expect(screen.getByTestId("theme")).toHaveTextContent("system")
  })

  it("reads a theme that was stored earlier", () => {
    stubLocalStorage({
      getItem: () => "dark",
      setItem: () => {},
    })
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>
    )
    expect(screen.getByTestId("theme")).toHaveTextContent("dark")
  })

  it("ignores a stored value that is not a theme", () => {
    stubLocalStorage({
      getItem: () => "chartreuse",
      setItem: () => {},
    })
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>
    )
    expect(screen.getByTestId("theme")).toHaveTextContent("system")
  })
})
