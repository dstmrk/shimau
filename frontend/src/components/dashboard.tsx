import * as React from "react"
import { useQueryClient } from "@tanstack/react-query"
import { LayersIcon, LogOutIcon, RefreshCwIcon } from "lucide-react"
import { toast } from "sonner"

import { LogsDialog } from "@/components/logs-dialog"
import {
  OperationConsole,
  type ActiveOperation,
} from "@/components/operation-console"
import { StackCard } from "@/components/stack-card"
import { ThemeToggle } from "@/components/theme-toggle"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { STACKS_QUERY_KEY, useStackAction, useStacks } from "@/hooks/use-stacks"
import { ApiError, api } from "@/lib/api"
import type { Identity, StackAction } from "@/lib/types"

// Both editors pull in CodeMirror, which is the largest thing shimau ships.
// Loading them on first use keeps the dashboard's first paint small; they
// stay mounted afterwards so the close animation is not cut short.
const ComposeDialog = React.lazy(() =>
  import("@/components/compose-dialog").then((m) => ({
    default: m.ComposeDialog,
  }))
)
const EnvDialog = React.lazy(() =>
  import("@/components/env-dialog").then((m) => ({ default: m.EnvDialog }))
)

export function Dashboard({
  identity,
  onSignedOut,
}: {
  identity: Identity
  onSignedOut: () => void
}) {
  const client = useQueryClient()
  const stacks = useStacks()
  const runAction = useStackAction()

  const [operation, setOperation] = React.useState<ActiveOperation | null>(null)
  const [logsFor, setLogsFor] = React.useState<string | null>(null)
  const [composeFor, setComposeFor] = React.useState<string | null>(null)
  const [envFor, setEnvFor] = React.useState<string | null>(null)
  const [composeLoaded, setComposeLoaded] = React.useState(false)
  const [envLoaded, setEnvLoaded] = React.useState(false)

  async function start(stack: string, action: StackAction) {
    try {
      const { operation_id } = await runAction.mutateAsync({ stack, action })
      setOperation({ id: operation_id, stack, action })
    } catch (error) {
      toast.error(
        error instanceof ApiError ? error.message : "Could not start the action"
      )
    }
  }

  async function signOut() {
    try {
      await api.logout()
    } finally {
      onSignedOut()
    }
  }

  // A stack the server reports as busy is one this browser may not have
  // started — another tab, or a reload mid-operation. Either way its buttons
  // stay disabled (spec §5.2).
  const busyStacks = new Set(
    (stacks.data ?? [])
      .filter((stack) => stack.active_operation_id)
      .map((stack) => stack.name)
  )

  return (
    <div className="mx-auto flex min-h-svh w-full max-w-3xl flex-col gap-6 p-6">
      <header className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-2.5">
          <LayersIcon className="size-5 shrink-0 text-primary" aria-hidden />
          <div>
            <h1 className="font-medium">shimau</h1>
            <p className="text-xs text-muted-foreground">
              Signed in as {identity.username}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            aria-label="Rescan the stacks directory"
            onClick={() =>
              client.invalidateQueries({ queryKey: STACKS_QUERY_KEY })
            }
          >
            <RefreshCwIcon
              className={stacks.isFetching ? "animate-spin" : undefined}
            />
          </Button>
          <ThemeToggle />
          <Button
            variant="ghost"
            size="icon"
            className="hover:text-destructive-emphasis"
            aria-label="Sign out"
            onClick={signOut}
          >
            <LogOutIcon />
          </Button>
        </div>
      </header>

      <main className="flex flex-col gap-3">
        {stacks.isPending && (
          <>
            <Skeleton className="h-28 w-full" />
            <Skeleton className="h-28 w-full" />
          </>
        )}

        {stacks.isError && (
          <Alert variant="destructive">
            <AlertTitle>Could not read the stacks directory</AlertTitle>
            <AlertDescription>
              {stacks.error instanceof ApiError
                ? stacks.error.message
                : "Unknown error"}
            </AlertDescription>
          </Alert>
        )}

        {stacks.data?.length === 0 && (
          <Alert>
            <AlertTitle>No stacks found</AlertTitle>
            <AlertDescription>
              A stack is a directory in the configured stacks directory holding
              exactly one of <code>compose.yaml</code>, <code>compose.yml</code>
              , <code>docker-compose.yaml</code> or{" "}
              <code>docker-compose.yml</code>.
            </AlertDescription>
          </Alert>
        )}

        {stacks.data?.map((stack) => (
          <StackCard
            key={stack.name}
            stack={stack}
            busy={busyStacks.has(stack.name)}
            onAction={(action) => start(stack.name, action)}
            onLogs={() => setLogsFor(stack.name)}
            onCompose={() => {
              setComposeLoaded(true)
              setComposeFor(stack.name)
            }}
            onEnv={() => {
              setEnvLoaded(true)
              setEnvFor(stack.name)
            }}
          />
        ))}
      </main>

      <OperationConsole
        operation={operation}
        onOpenChange={(open) => !open && setOperation(null)}
      />
      <LogsDialog
        stack={logsFor}
        onOpenChange={(open) => !open && setLogsFor(null)}
      />
      <React.Suspense fallback={null}>
        {composeLoaded && (
          <ComposeDialog
            stack={composeFor}
            onOpenChange={(open) => !open && setComposeFor(null)}
          />
        )}
        {envLoaded && (
          <EnvDialog
            stack={envFor}
            onOpenChange={(open) => !open && setEnvFor(null)}
          />
        )}
      </React.Suspense>
    </div>
  )
}
