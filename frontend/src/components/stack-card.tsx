import {
  FileTextIcon,
  KeyRoundIcon,
  PlayIcon,
  RotateCwIcon,
  ScrollTextIcon,
  SquareIcon,
  TriangleAlertIcon,
  DownloadIcon,
} from "lucide-react"

import { StatusBadge } from "@/components/status-badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import type { Stack, StackAction } from "@/lib/types"

/**
 * One stack, one row: every action it supports is reachable without drilling
 * into a detail page (spec §5.1).
 *
 * Emphasis carries the meaning, in four tiers: solid primary for the
 * affirmative lifecycle action, a destructive tint for the one that takes
 * services offline, outline for the actions that change something while the
 * stack stays up, ghost for the ones that only open a view.
 */
export function StackCard({
  stack,
  busy,
  onAction,
  onLogs,
  onCompose,
  onEnv,
}: {
  stack: Stack
  busy: boolean
  onAction: (action: StackAction) => void
  onLogs: () => void
  onCompose: () => void
  onEnv: () => void
}) {
  const ambiguous = stack.kind === "ambiguous"
  // Gates Restart and Update. Both end in a Compose call that would create or
  // start containers on a stack that is down — `restart` starts stopped ones,
  // and Update finishes with `up -d` — so neither is offered until the stack
  // is up. Starting a stack is what Start is for.
  const isUp = stack.status === "running" || stack.status === "partial"

  return (
    <Card>
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <h2 className="truncate font-medium">{stack.name}</h2>
            <p className="truncate font-mono text-xs text-muted-foreground">
              {ambiguous
                ? stack.compose_files?.join(" · ")
                : stack.compose_file}
            </p>
          </div>
          <StatusBadge status={stack.status} className="shrink-0" />
        </div>

        {ambiguous ? (
          <div className="flex items-start gap-2 text-xs text-destructive">
            <TriangleAlertIcon className="mt-0.5 size-3.5 shrink-0" />
            <p>
              Several Compose files in this directory. shimau will not act on it
              until exactly one is left.
            </p>
          </div>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {isUp ? (
              <Button
                variant="destructive"
                disabled={busy}
                onClick={() => onAction("stop")}
              >
                <SquareIcon data-icon="inline-start" />
                Stop
              </Button>
            ) : (
              <Button disabled={busy} onClick={() => onAction("start")}>
                <PlayIcon data-icon="inline-start" />
                Start
              </Button>
            )}
            <Button
              variant="outline"
              disabled={busy || !isUp}
              onClick={() => onAction("update")}
            >
              <DownloadIcon data-icon="inline-start" />
              Update
            </Button>
            <Button
              variant="outline"
              disabled={busy || !isUp}
              onClick={() => onAction("restart")}
            >
              <RotateCwIcon data-icon="inline-start" />
              Restart
            </Button>
            <Button variant="ghost" onClick={onLogs}>
              <ScrollTextIcon data-icon="inline-start" />
              Logs
            </Button>
            <Button variant="ghost" onClick={onCompose}>
              <FileTextIcon data-icon="inline-start" />
              Compose
            </Button>
            {stack.has_env_file && (
              <Button variant="ghost" onClick={onEnv}>
                <KeyRoundIcon data-icon="inline-start" />
                .env
              </Button>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
