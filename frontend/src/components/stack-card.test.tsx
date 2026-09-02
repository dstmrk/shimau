import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { describe, expect, it, vi } from "vitest"

import { StackCard } from "@/components/stack-card"
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

function renderCard(value: Stack, busy = false) {
  const onAction = vi.fn()
  render(
    <StackCard
      stack={value}
      busy={busy}
      onAction={onAction}
      onLogs={vi.fn()}
      onCompose={vi.fn()}
      onEnv={vi.fn()}
    />
  )
  return { onAction }
}

describe("StackCard", () => {
  it("offers Stop while the stack is running", () => {
    renderCard(stack({ status: "running" }))
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument()
    expect(screen.queryByRole("button", { name: "Start" })).toBeNull()
  })

  it("offers Start once the stack is stopped", () => {
    renderCard(stack({ status: "stopped" }))
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument()
  })

  it("sends the action the button promises", async () => {
    const { onAction } = renderCard(stack({ status: "stopped" }))
    await userEvent.click(screen.getByRole("button", { name: "Start" }))
    expect(onAction).toHaveBeenCalledWith("start")
  })

  it("keeps Logs and Compose reachable whatever the status", () => {
    renderCard(stack({ status: "not_created" }))
    expect(screen.getByRole("button", { name: "Logs" })).toBeEnabled()
    expect(screen.getByRole("button", { name: "Compose" })).toBeEnabled()
  })

  it("hides the .env button when the file does not exist", () => {
    renderCard(stack({ has_env_file: false }))
    expect(screen.queryByRole("button", { name: ".env" })).toBeNull()
  })

  it("shows the .env button when the file exists", () => {
    renderCard(stack({ has_env_file: true }))
    expect(screen.getByRole("button", { name: ".env" })).toBeInTheDocument()
  })

  it("disables lifecycle actions while an operation is in flight", () => {
    renderCard(stack({ status: "running" }), true)
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Update" })).toBeDisabled()
    expect(screen.getByRole("button", { name: "Restart" })).toBeDisabled()
  })

  it("cannot restart a stack that is not up", () => {
    renderCard(stack({ status: "stopped" }))
    expect(screen.getByRole("button", { name: "Restart" })).toBeDisabled()
  })

  it.each(["stopped", "not_created", "unknown"] as const)(
    "cannot update a %s stack, because up -d would start it",
    (status) => {
      renderCard(stack({ status }))
      expect(screen.getByRole("button", { name: "Update" })).toBeDisabled()
    }
  )

  it.each(["running", "partial"] as const)(
    "can update a %s stack",
    (status) => {
      renderCard(stack({ status }))
      expect(screen.getByRole("button", { name: "Update" })).toBeEnabled()
    }
  )

  it("refuses every action on an ambiguous stack", () => {
    renderCard(
      stack({
        kind: "ambiguous",
        compose_file: undefined,
        compose_files: ["compose.yaml", "docker-compose.yml"],
      })
    )
    expect(screen.queryByRole("button", { name: "Start" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Update" })).toBeNull()
    expect(screen.getByText(/Several Compose files/)).toBeInTheDocument()
  })

  it("gives the ambiguous-stack warning the same red as Stop", () => {
    renderCard(
      stack({
        kind: "ambiguous",
        compose_file: undefined,
        compose_files: ["compose.yaml", "docker-compose.yml"],
      })
    )
    const warning = screen
      .getByText(/Several Compose files/)
      .closest(".text-destructive-emphasis")
    expect(warning).not.toBeNull()
  })

  it("gives Stop the destructive emphasis and nothing else on the card", () => {
    renderCard(stack({ status: "running", has_env_file: true }))
    const stop = screen.getByRole("button", { name: "Stop" })
    expect(stop.classList.contains("text-destructive-emphasis")).toBe(true)
    for (const name of ["Update", "Restart", "Logs", "Compose", ".env"]) {
      const other = screen.getByRole("button", { name })
      expect(other.classList.contains("text-destructive-emphasis")).toBe(false)
    }
  })

  it("keeps Start on the primary emphasis rather than a colour of its own", () => {
    renderCard(stack({ status: "stopped" }))
    const start = screen.getByRole("button", { name: "Start" })
    expect(start.classList.contains("bg-primary")).toBe(true)
    expect(start.classList.contains("text-destructive-emphasis")).toBe(false)
  })

  it("shows which compose file the stack uses", () => {
    renderCard(stack({ compose_file: "docker-compose.yml" }))
    expect(screen.getByText("docker-compose.yml")).toBeInTheDocument()
  })
})
