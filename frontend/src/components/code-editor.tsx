import * as React from "react"
import CodeMirror, { type ReactCodeMirrorRef } from "@uiw/react-codemirror"
import { yaml } from "@codemirror/lang-yaml"
import { openSearchPanel } from "@codemirror/search"
import { oneDark } from "@codemirror/theme-one-dark"

import { useTheme } from "@/components/theme-provider"

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
      height="24rem"
      theme={resolvedTheme(theme) === "dark" ? oneDark : "light"}
      extensions={language === "yaml" ? [yaml()] : []}
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
