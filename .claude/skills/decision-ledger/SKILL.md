---
name: decision-ledger
description: Use when writing a pull request body, before asking for a merge. Covers the decision ledger — the calls made where the prompt or the spec was silent, ordered least-confident first — and the cost table that goes with it: lines added and removed split by production code, tests and documentation, plus every new structural surface such as an endpoint, an environment variable, a dependency or a database column.
---

# decision-ledger — hand over the decisions, not the diff

Reading a diff line by line does not keep pace with the rate code is produced,
and a review that cannot keep pace becomes a signature. The pull request body
carries what the diff cannot: what was decided, and what it cost.

The repository squash-merges, so the PR title and body **become the commit
message**. Both are finished before the merge, not after.

## (a) The ledger

Every call made where the prompt or the specification was silent. Ordered
**least confident first** — the reader's attention is finite and belongs on
the shakiest call, not the safest one.

Each entry:

- **Scenario** — plain prose, readable by someone who has not opened the diff.
- **Not specified** — what the spec or the prompt left open.
- **Decision** — what was done.
- **Confidence** — low / medium / high, and what would change it.

A decision deliberately left to the implementer is stated as delegated. A
freedom nobody noticed is a hole in the spec, and it belongs here rather than
in someone's memory.

## (b) The cost

```text
| Kind             | + | - |
| ---------------- | - | - |
| Production code  |   |   |
| Tests            |   |   |
| Documentation    |   |   |
```

Then every new **structural surface**, each on its own line: an API endpoint,
an environment variable, a database table or column, a dependency, a CI job, a
published image tag. These outlive the diff — a dependency added today is a
dependency someone audits for years.

A large net addition behind a small behavioural change is a finding to declare,
not a number to bury in the total.

## What this is not

The audit does not change code. If writing the ledger surfaces a problem, fix
it in a commit and write the ledger again; do not soften the entry to match
what shipped.
