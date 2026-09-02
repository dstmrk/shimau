import { describe, expect, it } from "vitest"

import { toDisplayLines, toDisplayText } from "@/lib/terminal"
import type { OutputLine } from "@/lib/types"

const ESC = "\u001b"
const BEL = "\u0007"

function stdout(...texts: string[]): OutputLine[] {
  return texts.map((text): OutputLine => ({ stream: "stdout", text }))
}

describe("toDisplayText", () => {
  it("strips the colour codes a container writes around its own output", () => {
    const raw =
      `${ESC}[37m[9/2/2026, 3:34:48 PM] ${ESC}[39m` +
      `${ESC}[36m[Nuki]${ESC}[39m restored 3 accessories from cache`
    expect(toDisplayText(raw)).toBe(
      "[9/2/2026, 3:34:48 PM] [Nuki] restored 3 accessories from cache"
    )
  })

  it("strips a 256-colour sequence and a reset", () => {
    expect(toDisplayText(`${ESC}[38;5;208mwarn${ESC}[0m`)).toBe("warn")
  })

  it("strips cursor and erase sequences", () => {
    expect(toDisplayText(`${ESC}[2K${ESC}[1Gdone`)).toBe("done")
  })

  it("strips an operating system command, terminator and all", () => {
    expect(toDisplayText(`${ESC}]0;window title${ESC}\\ready`)).toBe("ready")
    expect(toDisplayText(`${ESC}]0;bell terminated${BEL}ready`)).toBe("ready")
  })

  it("strips a charset designator and a lone escape", () => {
    expect(toDisplayText(`${ESC}(Bplain`)).toBe("plain")
    expect(toDisplayText(`half ${ESC}written`)).toBe("half written")
  })

  it("leaves text that only looks like an escape sequence alone", () => {
    expect(toDisplayText("[36m is not an escape without ESC")).toBe(
      "[36m is not an escape without ESC"
    )
  })

  it("keeps tabs but drops other control characters", () => {
    expect(toDisplayText(`a\tb${BEL}c`)).toBe("a\tbc")
  })

  it("drops a trailing carriage return", () => {
    expect(toDisplayText("done\r")).toBe("done")
  })

  it("shows only the last frame of a line redrawn with carriage returns", () => {
    expect(toDisplayText("10%\r50%\r100%\r")).toBe("100%")
  })

  it("returns an empty string for a line that was only escapes", () => {
    expect(toDisplayText(`${ESC}[2K\r`)).toBe("")
  })
})

describe("toDisplayLines", () => {
  it("cleans every line and keeps the stream it came from", () => {
    const lines: OutputLine[] = [
      { stream: "stderr", text: `${ESC}[31mfailed${ESC}[0m` },
    ]
    expect(toDisplayLines(lines)).toEqual([
      { stream: "stderr", text: "failed" },
    ])
  })

  it("keeps every line when progress collapsing is off", () => {
    const lines = stdout(
      " d21668d1c7b3 Extracting 49.58MB",
      " d21668d1c7b3 Extracting 61.83MB"
    )
    expect(toDisplayLines(lines)).toHaveLength(2)
  })

  it("collapses a layer's progress ticks into its latest one", () => {
    const lines = stdout(
      " nginx Pulling",
      " d21668d1c7b3 Extracting 49.58MB",
      " d21668d1c7b3 Extracting 61.83MB",
      " d21668d1c7b3 Pull complete"
    )
    expect(toDisplayLines(lines, { collapseProgress: true })).toEqual(
      stdout(" nginx Pulling", " d21668d1c7b3 Pull complete")
    )
  })

  it("gives each layer its own line, in the order they first appeared", () => {
    const lines = stdout(
      " aaaa1111 Downloading 1MB",
      " bbbb2222 Downloading 2MB",
      " aaaa1111 Download complete",
      " bbbb2222 Extracting 3MB"
    )
    expect(toDisplayLines(lines, { collapseProgress: true })).toEqual(
      stdout(" aaaa1111 Download complete", " bbbb2222 Extracting 3MB")
    )
  })

  it("keys a container line on its name, not on the word Container", () => {
    const lines = stdout(
      " Container home-web-1  Starting",
      " Container home-db-1  Starting",
      " Container home-web-1  Started"
    )
    expect(toDisplayLines(lines, { collapseProgress: true })).toEqual(
      stdout(" Container home-web-1  Started", " Container home-db-1  Starting")
    )
  })

  it("leaves output that is not a progress line untouched, repeats included", () => {
    const lines = stdout("waiting for lock", "waiting for lock")
    expect(toDisplayLines(lines, { collapseProgress: true })).toEqual(lines)
  })

  it("does not collapse a line whose status word is not Compose's", () => {
    const lines = stdout(
      "[Nuki] Citofono Nuki Opener",
      "[Nuki] Porta Nuki Lock"
    )
    expect(toDisplayLines(lines, { collapseProgress: true })).toEqual(lines)
  })

  it("collapses the cleaned text, not the raw bytes", () => {
    const lines = stdout(
      `${ESC}[34m aaaa1111 Downloading 1MB${ESC}[0m`,
      " aaaa1111 Downloading 2MB"
    )
    expect(toDisplayLines(lines, { collapseProgress: true })).toEqual(
      stdout(" aaaa1111 Downloading 2MB")
    )
  })
})
