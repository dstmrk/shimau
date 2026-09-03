import * as React from "react"
import CodeMirror, { type ReactCodeMirrorRef } from "@uiw/react-codemirror"
import { yaml } from "@codemirror/lang-yaml"
import { openSearchPanel } from "@codemirror/search"
import { oneDark } from "@codemirror/theme-one-dark"

import { useTheme } from "@/components/theme-provider"
import { dotenvHighlight, dotenvLanguage } from "@/lib/dotenv-highlight"

function resolvedTheme(theme: string) {
  if (theme === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light"
  }
  return theme
}

/**
 * The editor behind both file dialogs.
 *
 * `basicSetup` brings the search panel with it, so Ctrl/Cmd-F works inside
 * the file (spec §4.5); `onOpenSearch` exposes the same thing as a button for
 * anyone who does not know the shortcut.
 */
export function CodeEditor({
  value,
  onChange,
  language,
  readOnly = false,
  onOpenSearch,
}: {
  value: string
  onChange: (value: string) => void
  language: "yaml" | "plain"
  readOnly?: boolean
  onOpenSearch?: (open: () => void) => void
}) {
  // Grows with the file instead of reserving a fixed block: a three-line
  // `.env` in a 24rem editor is mostly empty space, and a long Compose file
  // still gets a scroll area rather than a page that scrolls under the
  // dialog.
  const { theme } = useTheme()
  const ref = React.useRef<ReactCodeMirrorRef>(null)

  React.useEffect(() => {
    onOpenSearch?.(() => {
      const view = ref.current?.view
      if (view) {
        openSearchPanel(view)
      }
    })
  }, [onOpenSearch])

  return (
    <CodeMirror
      ref={ref}
      value={value}
      onChange={onChange}
      readOnly={readOnly}
      minHeight="12rem"
      maxHeight="60vh"
      theme={resolvedTheme(theme) === "dark" ? oneDark : "light"}
      extensions={
        language === "yaml" ? [yaml()] : [dotenvLanguage, dotenvHighlight]
      }
      basicSetup={{
        lineNumbers: true,
        highlightActiveLine: !readOnly,
        foldGutter: language === "yaml",
        autocompletion: false,
      }}
      className="overflow-hidden rounded-md border text-xs"
    />
  )
}
