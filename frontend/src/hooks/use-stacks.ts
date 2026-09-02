import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"

import { api } from "@/lib/api"
import type { StackAction } from "@/lib/types"

export const STACKS_QUERY_KEY = ["stacks"] as const

/**
 * The stack list, re-scanned on an interval.
 *
 * Every refetch is a fresh directory scan plus a `docker compose ps` per
 * stack, which is exactly the point: there is no status database to go stale
 * (spec §4.2).
 */
export function useStacks() {
  return useQuery({
    queryKey: STACKS_QUERY_KEY,
    queryFn: api.listStacks,
    refetchInterval: 10_000,
    refetchOnWindowFocus: true,
  })
}

export function useStackAction() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ stack, action }: { stack: string; action: StackAction }) =>
      api.runAction(stack, action),
    onSettled: () => client.invalidateQueries({ queryKey: STACKS_QUERY_KEY }),
  })
}
