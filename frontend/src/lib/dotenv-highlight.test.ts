import { syntaxTree } from "@codemirror/language"
import { EditorState } from "@codemirror/state"
import { describe, expect, it } from "vitest"

import { dotenvLanguage } from "@/lib/dotenv-highlight"

function commentRanges(doc: string): string[] {
  const state = EditorState.create({
    doc,
    extensions: [dotenvLanguage],
  })
  const ranges: string[] = []
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name === "comment") {
        ranges.push(doc.slice(node.from, node.to))
      }
    },
  })
  return ranges
}

describe("dotenvLanguage", () => {
  it("tags a comment line", () => {
    expect(commentRanges("# database")).toEqual(["# database"])
  })

  it("tags an indented comment, matching maskEnv's rule", () => {
    expect(commentRanges("  # indented")).toEqual(["  # indented"])
  })

  it("does not tag a key=value line", () => {
    expect(commentRanges("DB_URL=postgres://u:p@h/db")).toEqual([])
  })

  it("does not tag a blank line", () => {
    expect(commentRanges("")).toEqual([])
  })

  it("does not tag a value that merely contains #", () => {
    expect(commentRanges("TOKEN=abc#def")).toEqual([])
  })

  it("tags only the comment lines in a mixed file", () => {
    const doc = ["# top", "KEY=value", "", "# another"].join("\n")
    expect(commentRanges(doc)).toEqual(["# top", "# another"])
  })
})
