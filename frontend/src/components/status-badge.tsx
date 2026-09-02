import { cn } from "@/lib/utils"
import type { StackStatus } from "@/lib/types"

const LABELS: Record<StackStatus, string> = {
  running: "Running",
  partial: "Partial",
  stopped: "Stopped",
  not_created: "Not created",
  unknown: "Unknown",
}

const DOT: Record<StackStatus, string> = {
  running: "bg-emerald-500",
  partial: "bg-amber-500",
  stopped: "bg-muted-foreground",
  not_created: "bg-muted-foreground/50",
  unknown: "bg-destructive",
}

export function StatusBadge({
  status,
  className,
}: {
  status: StackStatus
  className?: string
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 text-xs text-muted-foreground",
        className
      )}
    >
      <span aria-hidden className={cn("size-2 rounded-full", DOT[status])} />
      {LABELS[status]}
    </span>
  )
}
