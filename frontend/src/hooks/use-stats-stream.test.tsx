import { render } from "@testing-library/react"
import { act } from "react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { useStatsStream } from "@/hooks/use-stats-stream"
import type { ContainerStats } from "@/lib/types"

/** jsdom implements no `EventSource`; see `use-event-stream.test.tsx`. */
class FakeEventSource {
  static instances: FakeEventSource[] = []
  closed = false
  private listeners = new Map<string, (event: unknown) => void>()

  readonly url: string

  constructor(url: string) {
    this.url = url
    FakeEventSource.instances.push(this)
  }

  addEventListener(type: string, handler: (event: unknown) => void) {
    this.listeners.set(type, handler)
  }

  close() {
    this.closed = true
  }

  emit(type: string, data: unknown) {
    act(() => this.listeners.get(type)?.({ data: JSON.stringify(data) }))
  }
}

vi.stubGlobal("EventSource", FakeEventSource)

const CONTAINER: ContainerStats = {
  Name: "app-web-1",
  CPUPerc: "0.15%",
  MemUsage: "12MiB / 512MiB",
  MemPerc: "2.34%",
  NetIO: "1.2kB / 0B",
  BlockIO: "0B / 0B",
  PIDs: "3",
}

function open(url: string | null) {
  let seen: ContainerStats[] = []
  function Probe() {
    seen = useStatsStream(url)
    return null
  }
  const view = render(<Probe />)
  return {
    view,
    source: FakeEventSource.instances.at(-1),
    latest: () => seen,
  }
}

afterEach(() => {
  FakeEventSource.instances = []
})

describe("useStatsStream", () => {
  it("does not open a stream without a url", () => {
    open(null)
    expect(FakeEventSource.instances).toHaveLength(0)
  })

  it("returns the latest snapshot", () => {
    const { source, latest } = open("/api/stacks/app/stats/stream")
    source?.emit("stats", [CONTAINER])
    expect(latest()).toEqual([CONTAINER])
  })

  it("replaces the previous snapshot on the next event", () => {
    const { source, latest } = open("/api/stacks/app/stats/stream")
    source?.emit("stats", [CONTAINER])
    source?.emit("stats", [])
    expect(latest()).toEqual([])
  })

  it("closes the stream when the component goes away", () => {
    const { view, source } = open("/api/stacks/app/stats/stream")
    view.unmount()
    expect(source?.closed).toBe(true)
  })
})
