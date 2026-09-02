import type { OutputLine } from "@/lib/types"

/**
 * Turns raw command output into what a terminal would have shown.
 *
 * Two things stand between `docker compose` output and a readable `<pre>`:
 *
 * * Containers colour their own output. `--no-color` only silences Compose's
 *   service prefix; everything the container writes arrives verbatim, so a
 *   Homebridge line reaches the browser as `ESC[36m[Nuki]ESC[39m …` and
 *   renders as literal `[36m` noise.
 * * `--progress plain` prints every progress redraw on its own line, so one
 *   `pull` yields a hundred `d21668d1c7b3 Extracting 61.83MB` lines where a
 *   terminal rewrites a single one.
 *
 * This is presentation only. The API still relays exactly what Docker
 * printed, so running the same command by hand produces the same bytes.
 */

/**
 * An ESC-introduced sequence: an OSC (window title, hyperlink) up to its BEL
 * or string terminator, a CSI (colour, cursor movement, erase), an nF
 * sequence such as the charset designator `ESC ( B`, or a bare Fe/Fs escape.
 */
const ESCAPE_SEQUENCE =
  // eslint-disable-next-line no-control-regex -- terminal escapes are the subject
  /\u001b(?:\][\s\S]*?(?:\u0007|\u001b\\|$)|\[[0-?]*[ -/]*[@-~]|[ -/]+[0-~]|[@-Z\\-_])/g

/** C0 controls and DEL, tab excepted: a terminal shows none of them. */
// eslint-disable-next-line no-control-regex -- as above
const CONTROL = /[\u0000-\u0008\u000b-\u001f\u007f]/g

/**
 * Compose's status vocabulary, the one word that follows the thing it is
 * reporting on. A closed set on purpose: a status this list does not know
 * simply is not collapsed, which is a missed redraw rather than a swallowed
 * line. `Warning` and `Error` are deliberately absent — a failure message is
 * never overwritten by the next one.
 */
const PROGRESS_STATUSES = [
  "Already exists",
  "Download complete",
  "Downloading",
  "Extracting",
  "Pull complete",
  "Pulled",
  "Pulling fs layer",
  "Pulling",
  "Skipped",
  "Verifying Checksum",
  "Waiting",
  "Created",
  "Creating",
  "Healthy",
  "Recreate",
  "Recreated",
  "Removed",
  "Removing",
  "Restarted",
  "Restarting",
  "Running",
  "Started",
  "Starting",
  "Stopped",
  "Stopping",
]

/**
 * ` d21668d1c7b3 Extracting 49.58MB` → `d21668d1c7b3`, and
 * ` Container home-web-1  Started` → `Container home-web-1`: everything left
 * of the status is the thing being redrawn, and two lines about the same
 * thing occupy one row.
 *
 * Longest status first, so `Pulling fs layer` is not read as `Pulling`.
 */
const PROGRESS_LINE = new RegExp(
  `^\\s*(\\S.*?)\\s+(?:${[...PROGRESS_STATUSES]
    .sort((a, b) => b.length - a.length)
    .join("|")})\\b`
)

/** One output line as a terminal would display it. */
export function toDisplayText(text: string): string {
  // A carriage return means "start this line again": only the last frame was
  // ever visible, and a line that merely ends in one has nothing after it.
  const frames = text.split("\r")
  while (frames.length > 1 && frames[frames.length - 1] === "") {
    frames.pop()
  }
  return frames[frames.length - 1]
    .replace(ESCAPE_SEQUENCE, "")
    .replace(CONTROL, "")
}

/**
 * Cleans a transcript for display, optionally folding Compose's progress
 * redraws back onto one line each.
 *
 * Collapsing is opt-in because it only holds for Compose's own output: a
 * container is free to log two identical-looking lines, and the log viewer
 * must show both.
 */
export function toDisplayLines(
  lines: OutputLine[],
  { collapseProgress = false }: { collapseProgress?: boolean } = {}
): OutputLine[] {
  const displayed = lines.map((line) => ({
    ...line,
    text: toDisplayText(line.text),
  }))
  if (!collapseProgress) {
    return displayed
  }

  const collapsed: OutputLine[] = []
  const rows = new Map<string, number>()
  for (const line of displayed) {
    const key = PROGRESS_LINE.exec(line.text)?.[1]
    const row = key === undefined ? undefined : rows.get(key)
    if (row === undefined) {
      if (key !== undefined) {
        rows.set(key, collapsed.length)
      }
      collapsed.push(line)
    } else {
      collapsed[row] = line
    }
  }
  return collapsed
}
