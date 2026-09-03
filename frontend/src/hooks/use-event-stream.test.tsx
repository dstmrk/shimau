import { render } from "@testing-library/react"
import { act } from "react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { useEventStream, type StreamHandlers } from "@/hooks/use-event-stream"

/**
 * jsdom implements no `EventSource`, so the hook gets one that records what it
 * was asked to do and lets a test push events and connection states at it.
 */
class FakeEventSource {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2
  static instances: FakeEventSource[] = []

  readyState = FakeEventSource.OPEN
  closed = false
  onerror: (() => void) | null = null
  private listeners = new Map<string, (event: unknown) => void>()

  readonly url: string

  constructor(url: string) {
    // A constructor parameter property would not survive `erasableSyntaxOnly`,
    // which `npm run build` enforces and `npm run typecheck` does not.
    this.url = url
    FakeEventSource.instances.push(this)
  }

  addEventListener(type: string, handler: (event: unknown) => void) {
    this.listeners.set(type, handler)
  }

  close() {
    this.closed = true
    this.readyState = FakeEventSource.CLOSED
  }

  emit(type: string, data: unknown) {
    act(() => this.listeners.get(type)?.({ data: JSON.stringify(data) }))
  }

  fail(readyState: number) {
    this.readyState = readyState
    act(() => this.onerror?.())
  }
}

vi.stubGlobal("EventSource", FakeEventSource)

function open(handlers: StreamHandlers, url: string | null = "/api/stream") {
  function Probe() {
    useEventStream(url, handlers)
    return null
  }
  const view = render(<Probe />)
  return { view, source: FakeEventSource.instances.at(-1) }
}

afterEach(() => {
  FakeEventSource.instances = []
})

describe("useEventStream", () => {
  it("does not open a stream without a url", () => {
    open({ onLine: vi.fn() }, null)
    expect(FakeEventSource.instances).toHaveLength(0)
  })

  it("hands each line to the caller", () => {
    const onLine = vi.fn()
    const { source } = open({ onLine })
    source?.emit("line", { stream: "stdout", text: "Pulling" })
    expect(onLine).toHaveBeenCalledWith({ stream: "stdout", text: "Pulling" })
  })

  it("closes the stream on the finish event so the browser does not retry", () => {
    const onFinished = vi.fn()
    const { source } = open({ onLine: vi.fn(), onFinished })
    source?.emit("finished", { status: "succeeded" })
    expect(onFinished).toHaveBeenCalledWith({ status: "succeeded" })
    expect(source?.closed).toBe(true)
  })

  // The reason this branch exists: the server answers a reconnection by
  // replaying its whole buffer, so a caller that keeps what it already has
  // ends up showing the run twice.
  it("announces a retry rather than an error while the browser reconnects", () => {
    const onReconnecting = vi.fn()
    const onError = vi.fn()
    const { source } = open({ onLine: vi.fn(), onReconnecting, onError })
    source?.fail(FakeEventSource.CONNECTING)
    expect(onReconnecting).toHaveBeenCalled()
    expect(onError).not.toHaveBeenCalled()
  })

  it("reports an error once the browser has given up", () => {
    const onReconnecting = vi.fn()
    const onError = vi.fn()
    const { source } = open({ onLine: vi.fn(), onReconnecting, onError })
    source?.fail(FakeEventSource.CLOSED)
    expect(onError).toHaveBeenCalled()
    expect(onReconnecting).not.toHaveBeenCalled()
  })

  it("closes the stream when the component goes away", () => {
    const { view, source } = open({ onLine: vi.fn() })
    view.unmount()
    expect(source?.closed).toBe(true)
  })
})
