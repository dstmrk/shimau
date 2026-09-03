#!/usr/bin/env node
// Fails when a colour pair the UI actually renders drops below the contrast
// its role requires. The theme tokens live in frontend/src/index.css; the
// pairs below name where each one lands on screen.
//
// Text needs 4.5:1 (WCAG 2.2 §1.4.3, and every label in this app is small —
// 12px at font-medium, well under the 18.66px bold that would earn the 3:1
// exemption). A focus indicator needs 3:1 against what surrounds it (§1.4.11).

import { readFile } from "node:fs/promises"
import { fileURLToPath } from "node:url"
import { dirname, join } from "node:path"

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..")
const CSS = join(ROOT, "frontend/src/index.css")

/** A background built by compositing `token` at `alpha` over `over`. */
const tint = (token, alpha, over) => ({ token, alpha, over })

// Every pair is one thing a user looks at. `light`/`dark` override the shared
// definition when the variants differ.
const CHECKS = [
  {
    what: "Stop button label on its tint",
    fg: "destructive-emphasis",
    light: { bg: tint("destructive", 0.1, "card") },
    dark: { bg: tint("destructive", 0.2, "card") },
    min: 4.5,
  },
  {
    what: "Stop button label, hovered",
    fg: "destructive-emphasis",
    light: { bg: tint("destructive", 0.2, "card") },
    dark: { bg: tint("destructive", 0.3, "card") },
    min: 4.5,
  },
  {
    what: "Destructive text on a card (alert, ambiguous-stack warning)",
    fg: "destructive-emphasis",
    bg: "card",
    min: 4.5,
  },
  {
    what: "Destructive description text, which renders at 90%",
    fg: tint("destructive-emphasis", 0.9, "card"),
    bg: "card",
    min: 4.5,
  },
  {
    what: "Start button label on the primary fill",
    fg: "primary-foreground",
    bg: "primary",
    min: 4.5,
  },
  {
    what: "Muted text on the page",
    fg: "muted-foreground",
    bg: "background",
    min: 4.5,
  },
  {
    what: "Stderr line in the console/log pane (ConsoleOutput's bg-muted/15 over the dialog)",
    fg: "muted-foreground",
    bg: tint("muted", 0.15, "popover"),
    min: 4.5,
  },
  {
    what: "Focus indicator (focus-visible:border-ring) against the page",
    fg: "ring",
    bg: "background",
    min: 3,
  },
  {
    what: "Focus indicator against a card",
    fg: "ring",
    bg: "card",
    min: 3,
  },
]

// --- colour ---------------------------------------------------------------

function oklchToLinearSrgb(L, C, H) {
  const h = (H * Math.PI) / 180
  const a = C * Math.cos(h)
  const b = C * Math.sin(h)
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

const clamp = (x) => Math.min(1, Math.max(0, x))

const encode = (x) =>
  x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055

const decode = (x) => (x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4)

/**
 * Gamma-encoded sRGB, because that is the space a browser composites alpha in:
 * a translucent layer blends against the encoded value of what is behind it,
 * not against linear light. Blending in linear space instead reports ratios
 * around a point lower on a tint, which is the difference between passing and
 * failing here.
 */
function parseColour(value, where) {
  const m = /^oklch\(\s*([\d.]+%?)\s+([\d.]+)\s+([\d.]+)\s*\)$/.exec(value)
  if (!m) throw new Error(`${where}: cannot read colour ${JSON.stringify(value)}`)
  const raw = m[1]
  const L = raw.endsWith("%") ? Number(raw.slice(0, -1)) / 100 : Number(raw)
  // Out-of-gamut values are what the browser shows after clipping, so clip too
  // rather than reporting a ratio no screen can produce.
  return oklchToLinearSrgb(L, Number(m[2]), Number(m[3])).map((c) =>
    encode(clamp(c))
  )
}

const composite = (fg, bg, alpha) => fg.map((c, i) => c * alpha + bg[i] * (1 - alpha))

const luminance = (rgb) => {
  const [r, g, b] = rgb.map(decode)
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (hi + 0.05) / (lo + 0.05)
}

// --- tokens ---------------------------------------------------------------

/** Pulls the custom properties out of the `:root` and `.dark` blocks. */
function readThemes(css) {
  const themes = {}
  for (const [name, selector] of [
    ["light", ":root"],
    ["dark", ".dark"],
  ]) {
    const start = css.indexOf(`${selector} {`)
    if (start === -1) throw new Error(`index.css: no ${selector} block`)
    const end = css.indexOf("\n}", start)
    if (end === -1) throw new Error(`index.css: ${selector} block is not closed`)
    const block = css.slice(start + selector.length + 2, end)
    // A block that swallowed the next one would quietly mix two themes and
    // could report a pass that the browser does not render.
    if (block.includes("{")) {
      throw new Error(`index.css: ${selector} block runs past its closing brace`)
    }
    const tokens = {}
    for (const line of block.split("\n")) {
      const m = /^\s*--([a-z0-9-]+):\s*(.+?);\s*$/.exec(line)
      if (m) tokens[m[1]] = m[2]
    }
    themes[name] = tokens
  }
  return themes
}

function resolve(spec, tokens, theme) {
  if (spec === undefined) {
    throw new Error(`${theme}: a check names no colour — a per-theme override is missing`)
  }
  if (typeof spec === "string") {
    const value = tokens[spec]
    if (value === undefined) throw new Error(`${theme}: no --${spec} in index.css`)
    return parseColour(value, `${theme} --${spec}`)
  }
  const over = resolve(spec.over, tokens, theme)
  return composite(resolve(spec.token, tokens, theme), over, spec.alpha)
}

// --- run ------------------------------------------------------------------

const report = process.argv.includes("--report")
const themes = readThemes(await readFile(CSS, "utf8"))
const failures = []
let checked = 0

for (const [theme, tokens] of Object.entries(themes)) {
  for (const check of CHECKS) {
    const { fg, bg, min } = { ...check, ...(check[theme] ?? {}) }
    const ratio = contrast(resolve(fg, tokens, theme), resolve(bg, tokens, theme))
    checked += 1
    const short = ratio + 1e-9 < min
    if (report) {
      const mark = short ? "FAIL" : "ok  "
      console.log(
        `${mark} ${theme.padEnd(5)} ${ratio.toFixed(2).padStart(5)}:1 ` +
          `(needs ${min})  ${check.what}`
      )
    }
    if (short) {
      failures.push(
        `  ${theme}: ${check.what}\n` +
          `    ${ratio.toFixed(2)}:1, needs ${min}:1`
      )
    }
  }
}

if (failures.length > 0) {
  console.error(`Contrast check failed (${failures.length} of ${checked}):\n`)
  console.error(failures.join("\n"))
  console.error(
    "\nThe tokens are in frontend/src/index.css. Raising a ratio means moving\n" +
      "the token, not the pair: the pairs describe what the UI already renders."
  )
  process.exit(1)
}

console.log(`Contrast check passed: ${checked} pair(s) across 2 theme(s).`)
