import { render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { ConsoleOutput } from "@/components/console-output"
import type { OutputLine } from "@/lib/types"

const ESC = "\u001b"

function stdout(...texts: string[]): OutputLine[] {
  return texts.map((text): OutputLine => ({ stream: "stdout", text }))
}

describe("ConsoleOutput", () => {
  it("renders a coloured container line without its escape codes", () => {
    render(
      <ConsoleOutput
        lines={stdout(
          `${ESC}[36m[Nuki]${ESC}[39m restored 3 accessories from cache`
        )}
      />
    )
    expect(
      screen.getByText("[Nuki] restored 3 accessories from cache")
    ).toBeInTheDocument()
  })

  it("keeps repeated lines when collapsing is off", () => {
    render(
      <ConsoleOutput
        lines={stdout(
          " d21668d1c7b3 Extracting 49.58MB",
          " d21668d1c7b3 Extracting 61.83MB"
        )}
      />
    )
    expect(screen.getByText(/49\.58MB/)).toBeInTheDocument()
    expect(screen.getByText(/61\.83MB/)).toBeInTheDocument()
  })

  it("shows one row per layer when collapsing is on", () => {
    render(
      <ConsoleOutput
        collapseProgress
        lines={stdout(
          " d21668d1c7b3 Extracting 49.58MB",
          " d21668d1c7b3 Extracting 61.83MB"
        )}
      />
    )
    expect(screen.queryByText(/49\.58MB/)).not.toBeInTheDocument()
    expect(screen.getByText(/61\.83MB/)).toBeInTheDocument()
  })

  it("shows the empty message when there is nothing to display", () => {
    render(<ConsoleOutput lines={[]} emptyMessage="Nothing yet." />)
    expect(screen.getByText("Nothing yet.")).toBeInTheDocument()
  })
})
