import * as React from "react"

import type { OutputLine } from "@/lib/types"

export interface StreamHandlers {
  onLine: (line: OutputLine) => void
  onFinished?: (payload: unknown) => void
  onLagged?: (skipped: number) => void
  onError?: () => void
}

/**
 * Subscribes to one of the server's SSE endpoints for as long as `url` is set.
 *
 * Passing `null` closes the stream, which is what makes closing a dialog kill
 * the `docker compose logs --follow` behind it: the server drops the child
 * when the response body is dropped.
 */
export function useEventStream(url: string | null, handlers: StreamHandlers) {
  // Handlers are read through a ref so a re-render with a new closure does not
  // tear down and re-open the stream. The ref is refreshed in its own effect,
  // declared first so it has already run by the time the stream effect below
  // reads it.
  const ref = React.useRef(handlers)
  React.useEffect(() => {
    ref.current = handlers
  })

  React.useEffect(() => {
    if (!url) {
      return
    }

    const source = new EventSource(url)

    source.addEventListener("line", (event) => {
      try {
        ref.current.onLine(JSON.parse((event as MessageEvent).data))
      } catch {
        // A malformed frame is not worth tearing the stream down for.
      }
    })

    source.addEventListener("lagged", (event) => {
      try {
        const payload = JSON.parse((event as MessageEvent).data)
        ref.current.onLagged?.(payload.skipped ?? 0)
      } catch {
        ref.current.onLagged?.(0)
      }
    })

    source.addEventListener("finished", (event) => {
      let payload: unknown
      try {
        payload = JSON.parse((event as MessageEvent).data)
      } catch {
        payload = null
      }
      // The server has said its piece; closing here stops EventSource from
      // reconnecting and replaying the whole transcript.
      source.close()
      ref.current.onFinished?.(payload)
    })

    source.onerror = () => {
      // readyState CLOSED means the server ended the response and the browser
      // will not retry; anything else is a reconnect in progress.
      if (source.readyState === EventSource.CLOSED) {
        ref.current.onError?.()
      }
    }

    return () => source.close()
  }, [url])
}
