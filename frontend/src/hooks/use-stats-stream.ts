import * as React from "react"

import type { ContainerStats } from "@/lib/types"

/**
 * Subscribes to a stack's `stats/stream` endpoint for as long as `url` is
 * set, returning the latest snapshot.
 *
 * The stream has no end of its own — the server just keeps polling
 * `docker compose stats` and pushing a `stats` event every couple of
 * seconds — so closing the `EventSource` (unmounting, or clearing `url`) is
 * what stops the server's loop, the same way closing a log dialog kills
 * `docker compose logs --follow` behind it.
 */
export function useStatsStream(url: string | null): {
  containers: ContainerStats[]
  ready: boolean
} {
  const [containers, setContainers] = React.useState<ContainerStats[]>([])
  const [ready, setReady] = React.useState(false)

  // Resetting during render rather than in an effect, same as the dialogs
  // that reset their buffers when the stack prop changes: it avoids a frame
  // showing the previous stack's numbers before the effect below has run.
  const [shownUrl, setShownUrl] = React.useState(url)
  if (url !== shownUrl) {
    setShownUrl(url)
    setContainers([])
    setReady(false)
  }

  React.useEffect(() => {
    if (!url) {
      return
    }
    const source = new EventSource(url)
    source.addEventListener("stats", (event) => {
      try {
        setContainers(JSON.parse((event as MessageEvent).data))
        setReady(true)
      } catch {
        // A malformed frame is not worth tearing the stream down for.
      }
    })
    return () => source.close()
  }, [url])

  return { containers, ready }
}
