import * as React from "react"
import { useQueryClient } from "@tanstack/react-query"
import { CheckCircle2Icon, Loader2Icon, XCircleIcon } from "lucide-react"

import { ConsoleOutput } from "@/components/console-output"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useEventStream } from "@/hooks/use-event-stream"
import { STACKS_QUERY_KEY } from "@/hooks/use-stacks"
import { api, streams } from "@/lib/api"
import type { OperationSnapshot, OutputLine, StackAction } from "@/lib/types"

const VERB: Record<StackAction, string> = {
  start: "Starting",
  stop: "Stopping",
  restart: "Restarting",
  update: "Updating",
}

export interface ActiveOperation {
  id: string
  stack: string
  action: StackAction
}

/**
 * Live transcript of a Compose operation (spec §5.2).
 *
 * The dialog attaches to the operation's SSE stream, which replays whatever
 * has already been printed before switching to live output — so opening it
 * late, or reloading the page mid-`update`, still shows the whole run.
 */
export function OperationConsole({
  operation,
  onOpenChange,
}: {
  operation: ActiveOperation | null
  onOpenChange: (open: boolean) => void
}) {
  const client = useQueryClient()
  const [lines, setLines] = React.useState<OutputLine[]>([])
  const [outcome, setOutcome] = React.useState<
    OperationSnapshot["status"] | null
  >(null)
  const [failed, setFailed] = React.useState(false)

  const operationId = operation?.id ?? null

  // Adjusting state during render (React's "changing state when a prop
  // changes" pattern) so a newly opened console never shows the tail of the
  // previous operation.
  const [shownOperationId, setShownOperationId] = React.useState(operationId)
  if (operationId !== shownOperationId) {
    setShownOperationId(operationId)
    setLines([])
    setOutcome(null)
    setFailed(false)
  }

  useEventStream(operationId ? streams.operation(operationId) : null, {
    onLine: (line) => setLines((previous) => [...previous, line]),
    onLagged: (skipped) =>
      setLines((previous) => [
        ...previous,
        {
          stream: "stderr",
          text: `… ${skipped} lines dropped (output arrived faster than the browser read it)`,
        },
      ]),
    onReconnecting: () => setLines([]),
    onFinished: (payload) => {
      const status = (payload as { status?: OperationSnapshot["status"] })
        ?.status
      setOutcome(status ?? "succeeded")
      client.invalidateQueries({ queryKey: STACKS_QUERY_KEY })
    },
    onError: () => setFailed(true),
  })

  // The stream can drop before the finish event (a proxy timing out, a
  // restart). Falling back to a plain fetch keeps the dialog from sitting on
  // a spinner forever (spec §11).
  React.useEffect(() => {
    if (!failed || !operationId) {
      return
    }
    let cancelled = false
    api
      .operation(operationId)
      .then((snapshot) => {
        if (cancelled) {
          return
        }
        setLines(snapshot.lines)
        setOutcome(snapshot.status === "running" ? null : snapshot.status)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [failed, operationId])

  const running = outcome === null

  return (
    <Dialog open={operation !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {running && <Loader2Icon className="size-4 animate-spin" />}
            {outcome === "succeeded" && (
              <CheckCircle2Icon className="size-4 text-emerald-500" />
            )}
            {outcome === "failed" && (
              <XCircleIcon className="size-4 text-destructive" />
            )}
            {operation
              ? `${VERB[operation.action]} ${operation.stack}`
              : "Operation"}
          </DialogTitle>
          <DialogDescription>
            {running
              ? "Live output from Docker Compose. Closing leaves it running."
              : outcome === "succeeded"
                ? "Completed."
                : "Failed. The output below is the whole run."}
          </DialogDescription>
        </DialogHeader>

        <ConsoleOutput
          lines={lines}
          emptyMessage="Waiting for Docker Compose…"
          collapseProgress
        />

        <DialogFooter>
          {/* Closing never cancels: the operation runs on the server and the
              card keeps a way back to this transcript. Refusing to close was
              worse than useless — a `pull` from an unreachable registry held
              the whole UI behind a modal, and Escape closed it anyway. */}
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
