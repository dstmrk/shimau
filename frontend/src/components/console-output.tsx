import * as React from "react"

import { cn } from "@/lib/utils"
import type { OutputLine } from "@/lib/types"

/**
 * Monospaced output pane for Compose command output and container logs.
 *
 * Follows the tail while the reader is at the bottom, and stops following the
 * moment they scroll up — so reading back through a long `pull` is not fought
 * by every incoming line.
 */
export function ConsoleOutput({
  lines,
  emptyMessage = "No output yet.",
  className,
}: {
  lines: OutputLine[]
  emptyMessage?: string
  className?: string
}) {
  const viewport = React.useRef<HTMLDivElement>(null)
  const [following, setFollowing] = React.useState(true)

  React.useEffect(() => {
    const node = viewport.current
    if (!node || !following) {
      return
    }
    node.scrollTop = node.scrollHeight
  }, [lines, following])

  const handleScroll = React.useCallback(() => {
    const node = viewport.current
    if (!node) {
      return
    }
    const distanceFromBottom =
      node.scrollHeight - node.scrollTop - node.clientHeight
    setFollowing(distanceFromBottom < 32)
  }, [])

  return (
    <div className="relative">
      <div
        ref={viewport}
        onScroll={handleScroll}
        className={cn(
          "h-80 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs leading-relaxed",
          className
        )}
      >
        {lines.length === 0 ? (
          <p className="text-muted-foreground">{emptyMessage}</p>
        ) : (
          lines.map((line, index) => (
            <pre
              // Output lines have no identity of their own; the index is the
              // only stable key and the list is append-only.
              key={index}
              className={cn(
                "break-words whitespace-pre-wrap",
                line.stream === "stderr" && "text-destructive"
              )}
            >
              {line.text}
            </pre>
          ))
        )}
      </div>
      {!following && (
        <button
          type="button"
          onClick={() => setFollowing(true)}
          className="absolute right-3 bottom-3 rounded-md border bg-background/90 px-2 py-1 text-xs shadow-sm"
        >
          Follow output
        </button>
      )}
    </div>
  )
}
