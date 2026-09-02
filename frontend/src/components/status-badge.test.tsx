import { render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { StatusBadge } from "@/components/status-badge"
import type { StackStatus } from "@/lib/types"

describe("StatusBadge", () => {
  it.each<[StackStatus, string]>([
    ["running", "Running"],
    ["partial", "Partial"],
    ["stopped", "Stopped"],
    ["not_created", "Not created"],
    ["unknown", "Unknown"],
  ])("labels %s as %s", (status, label) => {
    render(<StatusBadge status={status} />)
    expect(screen.getByText(label)).toBeInTheDocument()
  })
})
