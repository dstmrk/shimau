/**
 * Validates the repository's own documentation against the repository.
 *
 * The failure this catches is the damaging one: a file gets renamed, the doc
 * that points at it does not, and the next reader — human or agent — is sent
 * somewhere that no longer exists. A stale map is worse than no map, so a dead
 * reference fails CI instead of quietly misleading.
 *
 * Run: node scripts/check-docs.mjs
 *
 * What is checked, and the contract the docs have to keep:
 *
 *  - Only INLINE code spans are scanned (`` `like/this` ``), never fenced
 *    blocks — directory trees and shell snippets stay illustrative.
 *  - A span counts as a repo path when it starts with one of the known top
 *    level directories and holds only path characters. Write each real path as
 *    its own span.
 *  - A trailing `:NN` or `:NN-MM` line reference is stripped before checking.
 *  - Spans containing `*`, `{` or `}` are treated as globs and skipped: they
 *    are patterns, not paths. Anything illustrative must be written that way.
 *  - A span naming a build artefact (`backend/target/…`, `frontend/dist/…`) is
 *    always an error: it exists on a developer machine with a warm build and
 *    never in CI, which is the nastiest way for this check to be useless.
 *  - Every `` skill `<name>` `` citation must resolve to a directory under
 *    `.claude/skills/`, and every skill directory must be cited at least once
 *    from CLAUDE.md or docs/architecture/ — otherwise a new skill silently
 *    drops out of the index.
 *  - A skill's YAML frontmatter is scanned too. Descriptions are plain text
 *    with no code spans, so bare tokens carrying a known prefix are validated
 *    there: a dead path in a description steers the skill's own activation.
 */

import { readdir, readFile, stat } from "node:fs/promises"
import { join, relative } from "node:path"

const ROOT = process.cwd()

const TOP_LEVEL_DIRS = [
  "backend",
  "frontend",
  "scripts",
  "docs",
  ".claude",
  ".github",
]

const PATH_PREFIX = new RegExp(`^(?:${TOP_LEVEL_DIRS.join("|")})/`)
const PATH_SHAPE = /^[A-Za-z0-9._/@()[\]-]+$/
const GENERATED = [/^backend\/target\//, /^frontend\/dist\//, /^frontend\/node_modules\//]

const INLINE_CODE = /`([^`\n]+)`/g
const FENCE = /^\s*(```|~~~)/
const SKILL_CITATION = /skills?\s+`([A-Za-z0-9_-]+)`/g

const errors = []

async function exists(path) {
  try {
    await stat(join(ROOT, path))
    return true
  } catch {
    return false
  }
}

async function walk(dir, filter) {
  const found = []
  let entries
  try {
    entries = await readdir(join(ROOT, dir), { withFileTypes: true })
  } catch {
    return found
  }
  for (const entry of entries) {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) {
      found.push(...(await walk(path, filter)))
    } else if (filter(path)) {
      found.push(path)
    }
  }
  return found
}

/** Inline code spans outside fenced blocks, with their line numbers. */
function inlineSpans(source) {
  const spans = []
  let inFence = false
  source.split("\n").forEach((line, index) => {
    if (FENCE.test(line)) {
      inFence = !inFence
      return
    }
    if (inFence) {
      return
    }
    for (const match of line.matchAll(INLINE_CODE)) {
      spans.push({ text: match[1], line: index + 1 })
    }
  })
  return spans
}

/** The frontmatter block of a SKILL.md, if it has one. */
function frontmatter(source) {
  if (!source.startsWith("---\n")) {
    return null
  }
  const end = source.indexOf("\n---", 4)
  return end === -1 ? null : source.slice(4, end)
}

function looksLikePath(token) {
  return PATH_PREFIX.test(token) && PATH_SHAPE.test(token)
}

function stripLineReference(token) {
  return token.replace(/:\d+(?:-\d+)?$/, "")
}

async function checkPathToken(token, file, line) {
  if (token.includes("*") || token.includes("{") || token.includes("}")) {
    return
  }
  const path = stripLineReference(token).replace(/\/$/, "")
  if (GENERATED.some((pattern) => pattern.test(path))) {
    errors.push(
      `${file}:${line}: \`${token}\` names a build artefact. It exists only ` +
        `after a build, so this check would pass locally and fail in CI. ` +
        `Write it as a glob or as plain text.`
    )
    return
  }
  if (!(await exists(path))) {
    errors.push(`${file}:${line}: \`${token}\` does not exist`)
  }
}

async function main() {
  const skillFiles = await walk(".claude/skills", (path) =>
    path.endsWith("SKILL.md")
  )
  const archDocs = await walk("docs", (path) => path.endsWith(".md"))
  const docs = ["CLAUDE.md", "README.md", ...archDocs, ...skillFiles]

  const skillDirs = new Set(
    skillFiles.map((path) => relative(".claude/skills", path).split("/")[0])
  )
  const citedSkills = new Set()

  for (const file of docs) {
    let source
    try {
      source = await readFile(join(ROOT, file), "utf8")
    } catch {
      errors.push(`${file}: listed for checking but could not be read`)
      continue
    }

    for (const { text, line } of inlineSpans(source)) {
      if (looksLikePath(text)) {
        await checkPathToken(text, file, line)
      }
    }

    let inFence = false
    source.split("\n").forEach((lineText, index) => {
      if (FENCE.test(lineText)) {
        inFence = !inFence
        return
      }
      if (inFence) {
        return
      }
      for (const match of lineText.matchAll(SKILL_CITATION)) {
        const name = match[1]
        citedSkills.add(name)
        if (!skillDirs.has(name)) {
          errors.push(
            `${file}:${index + 1}: skill \`${name}\` does not resolve to a ` +
              `directory under .claude/skills/`
          )
        }
      }
    })

    if (file.endsWith("SKILL.md")) {
      const header = frontmatter(source)
      if (header === null) {
        errors.push(`${file}: missing YAML frontmatter`)
      } else {
        if (!/^name:\s*\S+/m.test(header)) {
          errors.push(`${file}: frontmatter has no \`name\``)
        }
        if (!/^description:\s*\S+/m.test(header)) {
          errors.push(`${file}: frontmatter has no \`description\``)
        }
        const directory = relative(".claude/skills", file).split("/")[0]
        const declared = header.match(/^name:\s*(\S+)/m)?.[1]
        if (declared && declared !== directory) {
          errors.push(
            `${file}: frontmatter name \`${declared}\` does not match its ` +
              `directory \`${directory}\``
          )
        }
        for (const token of header.split(/[\s,;()]+/)) {
          const cleaned = token.replace(/[.,:;]+$/, "")
          if (looksLikePath(cleaned)) {
            await checkPathToken(cleaned, file, 1)
          }
        }
      }
    }
  }

  for (const skill of skillDirs) {
    if (!citedSkills.has(skill)) {
      errors.push(
        `.claude/skills/${skill}/: no document cites this skill. Add it to ` +
          `CLAUDE.md so the index cannot go stale.`
      )
    }
  }

  if (errors.length > 0) {
    console.error("Documentation references are out of date:\n")
    for (const error of errors) {
      console.error(`  ${error}`)
    }
    console.error(`\n${errors.length} problem(s).`)
    process.exit(1)
  }

  console.log(
    `Documentation check passed: ${docs.length} file(s), ${skillDirs.size} skill(s).`
  )
}

await main()
