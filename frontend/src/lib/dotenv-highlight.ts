import {
  HighlightStyle,
  StreamLanguage,
  syntaxHighlighting,
} from "@codemirror/language"
import { tags } from "@lezer/highlight"

/**
 * `.env` files have no CodeMirror language package of their own, and pulling
 * one in for a single token distinction (a comment line vs. everything else)
 * would be a lot of dependency for very little UI. A `StreamLanguage` that
 * only recognises `#` lines is the minimal implementation that satisfies the
 * request, and matches the comment rule `maskEnv` already uses: optional
 * leading whitespace, then `#`.
 */
export const dotenvLanguage = StreamLanguage.define<null>({
  token(stream) {
    if (stream.sol() && stream.match(/^\s*#.*$/)) {
      return "comment"
    }
    stream.skipToEnd()
    return null
  },
})

/**
 * Applied regardless of the active theme (`oneDark` carries its own
 * highlight style, but the plain "light" theme string does not), so comment
 * lines read consistently in both.
 */
export const dotenvHighlight = syntaxHighlighting(
  HighlightStyle.define([
    { tag: tags.comment, color: "var(--muted-foreground)" },
  ])
)
