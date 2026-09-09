import * as React from "react"
import { CopyIcon, KeyRoundIcon, Trash2Icon } from "lucide-react"
import { toast } from "sonner"

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { ApiError, api } from "@/lib/api"
import type { ApiToken, Capability } from "@/lib/types"

const CAPABILITIES: { value: Capability; title: string; blurb: string }[] = [
  {
    value: "read",
    title: "Read",
    blurb: "Stacks, status, logs, resource usage, Compose files, operations.",
  },
  {
    value: "operate",
    title: "Operate",
    blurb: "Everything Read can do, plus start, stop, restart and update.",
  },
]

function when(seconds: number | null): string {
  if (seconds === null) {
    return "never used"
  }
  return new Date(seconds * 1000).toLocaleString()
}

/**
 * API token management.
 *
 * The secret appears once, in [`NewToken`], because only its SHA-256 was
 * stored and shimau cannot show it again. Nothing here can be reached with a
 * token: the endpoints behind it are session-only, so a token cannot mint
 * itself a wider one.
 */
export function TokensDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const [tokens, setTokens] = React.useState<ApiToken[] | null>(null)
  const [label, setLabel] = React.useState("")
  const [capability, setCapability] = React.useState<Capability>("read")
  const [creating, setCreating] = React.useState(false)
  const [created, setCreated] = React.useState<string | null>(null)

  // Reset when the dialog opens, adjusted during render rather than in an
  // effect: an effect would paint the previous session's tokens first.
  const [wasOpen, setWasOpen] = React.useState(open)
  if (open !== wasOpen) {
    setWasOpen(open)
    if (open) {
      setTokens(null)
      setLabel("")
      setCapability("read")
      setCreated(null)
    }
  }

  React.useEffect(() => {
    if (!open) {
      return
    }
    let cancelled = false
    api
      .listTokens()
      .then((list) => !cancelled && setTokens(list))
      .catch((error: unknown) => {
        if (!cancelled) {
          setTokens([])
          toast.error(
            error instanceof ApiError
              ? error.message
              : "Could not list API tokens"
          )
        }
      })
    return () => {
      cancelled = true
    }
  }, [open])

  async function create(event: React.FormEvent) {
    event.preventDefault()
    setCreating(true)
    try {
      const token = await api.createToken(label.trim(), capability)
      setCreated(token.secret)
      setTokens((current) => [token, ...(current ?? [])])
      setLabel("")
    } catch (error) {
      toast.error(
        error instanceof ApiError ? error.message : "Could not create the token"
      )
    } finally {
      setCreating(false)
    }
  }

  async function revoke(token: ApiToken) {
    try {
      await api.revokeToken(token.id)
      setTokens((current) => (current ?? []).filter((t) => t.id !== token.id))
      toast.success(`${token.label} revoked`)
    } catch (error) {
      toast.error(
        error instanceof ApiError ? error.message : "Could not revoke the token"
      )
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>API tokens</DialogTitle>
          <DialogDescription>
            For clients that are not a browser. A token reads your stacks and,
            with Operate, runs the four lifecycle actions. No token can edit a
            Compose file or a <code>.env</code>.
          </DialogDescription>
        </DialogHeader>

        {created && <NewToken secret={created} />}

        <form onSubmit={create} className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="token-label">Label</Label>
            <Input
              id="token-label"
              value={label}
              maxLength={64}
              placeholder="What holds this token"
              onChange={(event) => setLabel(event.target.value)}
            />
          </div>

          <fieldset className="flex flex-col gap-1.5">
            <legend className="mb-1.5 text-sm font-medium">Capability</legend>
            <div className="grid gap-2 sm:grid-cols-2">
              {CAPABILITIES.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  aria-pressed={capability === option.value}
                  onClick={() => setCapability(option.value)}
                  className={`rounded-md border p-3 text-left text-sm transition-colors ${
                    capability === option.value
                      ? "border-primary bg-primary/5"
                      : "border-input hover:bg-accent"
                  }`}
                >
                  <span className="font-medium">{option.title}</span>
                  <span className="mt-0.5 block text-xs text-muted-foreground">
                    {option.blurb}
                  </span>
                </button>
              ))}
            </div>
          </fieldset>

          <Button
            type="submit"
            disabled={creating || label.trim().length === 0}
            className="self-start"
          >
            <KeyRoundIcon data-icon="inline-start" />
            {creating ? "Creating…" : "Create token"}
          </Button>
        </form>

        <div className="flex flex-col gap-2">
          {tokens === null && (
            <p className="text-sm text-muted-foreground">Loading…</p>
          )}
          {tokens?.length === 0 && (
            <p className="text-sm text-muted-foreground">
              No tokens yet. The browser session is the only way in.
            </p>
          )}
          {tokens?.map((token) => (
            <div
              key={token.id}
              className="flex items-center justify-between gap-3 rounded-md border p-3"
            >
              <div className="min-w-0">
                <p className="truncate text-sm font-medium">{token.label}</p>
                <p className="text-xs text-muted-foreground">
                  {token.capability} · created{" "}
                  {new Date(token.created_at * 1000).toLocaleDateString()} ·{" "}
                  {when(token.last_used_at)}
                </p>
              </div>
              <Button
                variant="ghost"
                size="icon"
                aria-label={`Revoke ${token.label}`}
                className="hover:text-destructive-emphasis"
                onClick={() => void revoke(token)}
              >
                <Trash2Icon />
              </Button>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}

/**
 * The token, shown once.
 *
 * `navigator.clipboard` is absent on a plain-HTTP origin, which is a supported
 * way to run shimau, so the value stays selectable text and the copy button is
 * an extra rather than the only way to get it.
 */
function NewToken({ secret }: { secret: string }) {
  async function copy() {
    try {
      await navigator.clipboard.writeText(secret)
      toast.success("Token copied")
    } catch {
      toast.error("Could not copy. Select the token and copy it by hand.")
    }
  }

  return (
    <Alert>
      <AlertTitle>Copy this token now</AlertTitle>
      <AlertDescription className="flex flex-col gap-2">
        <span>
          shimau stored only its hash and cannot show it again. Treat it like
          the administrator password: it controls Docker.
        </span>
        <span className="flex items-center gap-2">
          <code className="min-w-0 flex-1 overflow-x-auto rounded bg-muted px-2 py-1 font-mono text-xs">
            {secret}
          </code>
          <Button
            variant="outline"
            size="icon"
            aria-label="Copy the token"
            onClick={copy}
          >
            <CopyIcon />
          </Button>
        </span>
      </AlertDescription>
    </Alert>
  )
}
