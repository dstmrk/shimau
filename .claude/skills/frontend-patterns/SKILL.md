---
name: frontend-patterns
description: Use when writing or changing React code under frontend/src — components, hooks, dialogs, the API client, the shadcn/ui setup, the theme, or the Vite build. Covers the shadcn preset this project is pinned to and how to add components, adjusting state during render instead of in an effect (React 19 lint rules), keeping refs out of render, lazy-loading the CodeMirror editors, the typed API client and its ApiError, and the design constraints the spec puts on the UI.
---

# frontend-patterns — React 19, Vite, shadcn/ui

`frontend/` is a plain Vite SPA. No router: the app is a login screen and one
dashboard, and dialogs carry everything else. Adding a router would be a
dependency in search of a requirement.

## shadcn/ui

The project was initialised from a shadcn preset, recorded in
`frontend/components.json`:

```text
style base-mira · baseColor mist · theme sky · font geist · radius small · lucide
```

Add components with the CLI, never by hand:

```bash
cd frontend && npx shadcn@latest add <component>
```

Generated files land in `frontend/src/components/ui/` and are treated as
upstream code: `frontend/eslint.config.js` disables
`react-refresh/only-export-components` for that directory rather than
re-authoring files the next `shadcn add` would conflict with. Two edits to
generated files were necessary and are the exceptions: `sonner.tsx` reads the
local theme provider instead of `next-themes`, and the theme provider's global
"press d to toggle dark mode" listener was removed — a bare keypress flipping
the theme in an app full of editors is a surprise, not a feature.

Delete a generated component that nothing imports. Unused UI primitives are
bundle weight and lint noise.

## React 19 rules this codebase follows

**Adjust state during render, not in an effect.** When a dialog's `stack` prop
changes, resetting its buffers in a `useEffect` trips
`react-hooks` (cascading renders) and shows one frame of the previous stack's
content. The pattern used throughout:

```tsx
const [shownStack, setShownStack] = React.useState(stack)
if (stack !== shownStack) {
  setShownStack(stack)
  setLines([])
}
```

**Never assign a ref during render.** `use-event-stream.ts` refreshes its
handler ref in its own effect, declared *before* the effect that opens the
stream so the ordering is right. The point of the ref is that a new handler
closure on every render must not tear down and re-open the `EventSource`.

**Effects that fetch take a `cancelled` flag** and check it before every
`setState`, so a dialog reopened on another stack does not get the first
response written into it.

## Lazy-load the editors

`ComposeDialog` and `EnvDialog` pull in CodeMirror, which is larger than the
rest of the app combined. `frontend/src/components/dashboard.tsx` loads them
with `React.lazy` on first open and keeps them mounted afterwards, so the exit
animation still runs. Anything else that drags in a heavy dependency belongs
behind the same treatment.

## The API client

`frontend/src/lib/api.ts` is the only place that calls `fetch`. Every failure
becomes an `ApiError` carrying `status`, `code`, `details` and
`retryAfterSecs`, so callers branch on `code` rather than parsing messages —
`validation_failed` renders the Compose validator output verbatim,
`rate_limited` renders the wait.

An unreachable server becomes `ApiError` with `code: "network"` too. A raw
`TypeError` escaping into a component is how a UI ends up stuck on a spinner.

## What the UI must not become

Spec §5.3, and it is a real constraint: no gradients, no mascots, no dashboard
widgets, no second component library. Compact rows, every action on the card,
no navigation levels between the operator and Stop.
