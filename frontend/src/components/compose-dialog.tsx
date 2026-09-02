import * as React from "react"
import { useQueryClient } from "@tanstack/react-query"
import { SearchIcon } from "lucide-react"
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
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { STACKS_QUERY_KEY } from "@/hooks/use-stacks"
import { ApiError, api } from "@/lib/api"

/**
 * Compose file editor.
 *
 * The filename is shown and never changed: shimau edits the file the stack
 * already uses, whichever of the four supported names that is (spec §4.5).
 * A rejected save leaves the file on disk untouched, and the validator output
 * is shown verbatim rather than collapsed into "invalid".
 */
export function ComposeDialog({
  stack,
  onOpenChange,
}: {
  stack: string | null
  onOpenChange: (open: boolean) => void
}) {
  const client = useQueryClient()
  const [filename, setFilename] = React.useState("")
  const [content, setContent] = React.useState("")
  const [original, setOriginal] = React.useState("")
  const [loading, setLoading] = React.useState(false)
  const [saving, setSaving] = React.useState(false)
  const [validationError, setValidationError] = React.useState<string | null>(
    null
  )
  const openSearch = React.useRef<(() => void) | null>(null)

  const [shownStack, setShownStack] = React.useState(stack)
  if (stack !== shownStack) {
    setShownStack(stack)
    setValidationError(null)
    setLoading(stack !== null)
  }

  React.useEffect(() => {
    if (!stack) {
      return
    }
    let cancelled = false
    api
      .readCompose(stack)
      .then((file) => {
        if (cancelled) {
          return
        }
        setFilename(file.filename)
        setContent(file.content)
        setOriginal(file.content)
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          toast.error(
            error instanceof ApiError
              ? error.message
              : "Could not read the file"
          )
          onOpenChange(false)
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
  }, [stack, onOpenChange])

  const dirty = content !== original

  async function save() {
    if (!stack) {
      return
    }
    setSaving(true)
    setValidationError(null)
    try {
      const saved = await api.writeCompose(stack, content)
      setOriginal(saved.content)
      toast.success(`${saved.filename} saved`)
      client.invalidateQueries({ queryKey: STACKS_QUERY_KEY })
    } catch (error) {
      if (error instanceof ApiError && error.code === "validation_failed") {
        setValidationError(error.details ?? error.message)
      } else {
        toast.error(
          error instanceof ApiError ? error.message : "Could not save the file"
        )
      }
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={stack !== null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>
            {stack} — {filename || "compose file"}
          </DialogTitle>
          <DialogDescription>
            Saved only if <code>docker compose config</code> accepts it. The
            previous version is kept as{" "}
            <code>{filename ? `${filename}.bak` : "….bak"}</code>.
          </DialogDescription>
        </DialogHeader>

        {validationError && (
          <Alert variant="destructive">
            <AlertTitle>Not saved — Compose rejected the file</AlertTitle>
            <AlertDescription>
              <pre className="max-h-40 overflow-auto whitespace-pre-wrap">
                {validationError}
              </pre>
            </AlertDescription>
          </Alert>
        )}

        {loading ? (
          <div className="h-96 text-sm text-muted-foreground">Loading…</div>
        ) : (
          <CodeEditor
            value={content}
            onChange={setContent}
            language="yaml"
            onOpenSearch={(open) => {
              openSearch.current = open
            }}
          />
        )}

        <DialogFooter className="sm:justify-between">
          <Button
            variant="ghost"
            onClick={() => openSearch.current?.()}
            disabled={loading}
          >
            <SearchIcon data-icon="inline-start" />
            Search
          </Button>
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              {dirty ? "Discard" : "Close"}
            </Button>
            <Button onClick={save} disabled={!dirty || saving || loading}>
              {saving ? "Validating…" : "Save"}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
