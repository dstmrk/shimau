import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { useStatsStream } from "@/hooks/use-stats-stream"
import { streams } from "@/lib/api"

/**
 * Per-container CPU, memory, network and block I/O, polled from
 * `docker compose stats` every couple of seconds while the dialog is open.
 *
 * Closing the dialog is what stops the server's polling loop, the same way
 * closing the logs dialog kills `docker compose logs --follow` behind it.
 */
export function StatsDialog({
  stack,
  onOpenChange,
}: {
  stack: string | null
  onOpenChange: (open: boolean) => void
}) {
  const { containers, ready } = useStatsStream(
    stack ? streams.stats(stack) : null
  )

  return (
    <Dialog open={stack !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>Stats — {stack}</DialogTitle>
          <DialogDescription>
            Live resource usage per container.
          </DialogDescription>
        </DialogHeader>

        {!ready ? (
          <div className="space-y-2" aria-label="Loading stats">
            {Array.from({ length: 3 }, (_, i) => (
              <Skeleton key={i} className="h-8 w-full" />
            ))}
          </div>
        ) : containers.length === 0 ? (
          <p className="rounded-md border bg-muted/15 p-3 text-sm text-muted-foreground">
            No running containers to report on.
          </p>
        ) : (
          <div className="overflow-auto rounded-md border">
            <table className="w-full text-left text-xs">
              <thead className="bg-muted/30 text-muted-foreground">
                <tr>
                  <th className="p-2 font-medium">Container</th>
                  <th className="p-2 font-medium">CPU</th>
                  <th className="p-2 font-medium">Memory</th>
                  <th className="p-2 font-medium">Net I/O</th>
                  <th className="p-2 font-medium">Block I/O</th>
                  <th className="p-2 font-medium">PIDs</th>
                </tr>
              </thead>
              <tbody>
                {containers.map((container) => (
                  <tr key={container.Name} className="border-t">
                    <td className="p-2 font-mono">{container.Name}</td>
                    <td className="p-2 tabular-nums">{container.CPUPerc}</td>
                    <td className="p-2 tabular-nums">
                      {container.MemUsage}
                      <span className="text-muted-foreground">
                        {" "}
                        ({container.MemPerc})
                      </span>
                    </td>
                    <td className="p-2 tabular-nums">{container.NetIO}</td>
                    <td className="p-2 tabular-nums">{container.BlockIO}</td>
                    <td className="p-2 tabular-nums">{container.PIDs}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
