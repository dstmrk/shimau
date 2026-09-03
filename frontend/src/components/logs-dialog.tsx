import * as React from "react"

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
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { useEventStream } from "@/hooks/use-event-stream"
import { streams } from "@/lib/api"
import type { OutputLine } from "@/lib/types"

const TAIL = 300

/**
 * Container logs, followed over SSE.
 *
 * A stopped stack is not an error: Compose returns whatever exists and the
 * stream ends, which the dialog reports rather than hanging (spec §4.4).
 */
export function LogsDialog({
  stack,
  onOpenChange,
}: {
  stack: string | null
  onOpenChange: (open: boolean) => void
}) {
  const [lines, setLines] = React.useState<OutputLine[]>([])
  const [ended, setEnded] = React.useState(false)
  const [following, setFollowing] = React.useState(true)

  // Resetting during render rather than in an effect: React's documented way
  // of adjusting state when a prop changes, and it avoids a frame showing the
  // previous stack's output.
  const [shownStack, setShownStack] = React.useState(stack)
  if (stack !== shownStack) {
    setShownStack(stack)
    setLines([])
    setEnded(false)
    setFollowing(true)
  }

  useEventStream(stack && following ? streams.logs(stack, TAIL) : null, {
    onLine: (line) => setLines((previous) => [...previous, line]),
    onReconnecting: () => setLines([]),
    onFinished: () => setEnded(true),
    onError: () => setEnded(true),
  })

  return (
    <Dialog open={stack !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>Logs — {stack}</DialogTitle>
          <DialogDescription>
            Last {TAIL} lines
            {ended
              ? ". The stream ended: the stack has no running containers."
              : following
                ? ", following."
                : ", paused."}
          </DialogDescription>
        </DialogHeader>

        <ConsoleOutput
          lines={lines}
          emptyMessage="No log output. The stack may not be running."
        />

        <DialogFooter className="sm:justify-between">
          <div className="flex items-center gap-2">
            <Switch
              id="follow-logs"
              checked={following}
              onCheckedChange={setFollowing}
            />
            <Label htmlFor="follow-logs">Follow</Label>
          </div>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
