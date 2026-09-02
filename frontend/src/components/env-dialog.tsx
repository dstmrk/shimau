import * as React from "react"
import { EyeIcon, EyeOffIcon } from "lucide-react"
import { toast } from "sonner"

import { CodeEditor } from "@/components/code-editor"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { ApiError, api } from "@/lib/api"
import { maskEnv } from "@/lib/env-mask"

/**
 * `.env` editor.
 *
 * Values are hidden by default and the editor is read-only until they are
 * revealed, so a masked buffer can never be saved over the real secrets
 * (spec §4.6). The file is written back byte for byte.
 */
export function EnvDialog({
  stack,
  onOpenChange,
}: {
  stack: string | null
  onOpenChange: (open: boolean) => void
}) {
  const [content, setContent] = React.useState("")
  const [original, setOriginal] = React.useState("")
  const [revealed, setRevealed] = React.useState(false)
  const [loading, setLoading] = React.useState(false)
  const [saving, setSaving] = React.useState(false)

  // Held in a ref, not read from the closure: the parent passes a new
  // callback identity on every render, and listing it in the effect deps
  // below would re-run the fetch on every dashboard poll — overwriting
  // whatever the user had typed.
  const close = React.useRef(onOpenChange)
  React.useEffect(() => {
    close.current = onOpenChange
  })

  const [shownStack, setShownStack] = React.useState(stack)
  if (stack !== shownStack) {
    setShownStack(stack)
    setRevealed(false)
    setContent("")
    setOriginal("")
    setLoading(stack !== null)
  }

  React.useEffect(() => {
    if (!stack) {
      return
    }
    let cancelled = false
    api
      .readEnv(stack)
      .then((file) => {
        if (cancelled) {
          return
        }
        setContent(file.content)
        setOriginal(file.content)
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          toast.error(
            error instanceof ApiError ? error.message : "Could not read .env"
          )
          close.current(false)
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false)
        }
      })
    return () => {
      cancelled = true
    }
  }, [stack])

  const dirty = content !== original

  async function save() {
    if (!stack) {
      return
    }
    setSaving(true)
    try {
      const saved = await api.writeEnv(stack, content)
      setOriginal(saved.content)
      toast.success(".env saved")
    } catch (error) {
      toast.error(
        error instanceof ApiError ? error.message : "Could not save .env"
      )
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={stack !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{stack} — .env</DialogTitle>
          <DialogDescription>
            Written back exactly as typed. Values are hidden until you reveal
            them.
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="h-96 text-sm text-muted-foreground">Loading…</div>
        ) : (
          <CodeEditor
            value={revealed ? content : maskEnv(content)}
            onChange={setContent}
            language="plain"
            readOnly={!revealed}
          />
        )}

        <DialogFooter className="sm:justify-between">
          <Button variant="ghost" onClick={() => setRevealed((v) => !v)}>
            {revealed ? (
              <EyeOffIcon data-icon="inline-start" />
            ) : (
              <EyeIcon data-icon="inline-start" />
            )}
            {revealed ? "Hide values" : "Reveal values to edit"}
          </Button>
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              {dirty ? "Discard" : "Close"}
            </Button>
            <Button
              onClick={save}
              disabled={!revealed || !dirty || saving || loading}
            >
              {saving ? "Saving…" : "Save"}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
