---
name: frontend-patterns
description: Use when writing or changing React code under frontend/src — components, hooks, dialogs, the API client, the shadcn/ui setup, the theme, or the Vite build. Covers the shadcn preset this project is pinned to and how to add components, adjusting state during render instead of in an effect (React 19 lint rules), keeping refs out of render, why a parent-supplied callback must never sit in an effect dependency array, lazy-loading the CodeMirror editors, the typed API client and its ApiError, testing components that mount CodeMirror under jsdom, and the design constraints the spec puts on the UI.
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

A third edit to a generated file is now in place, in `button.tsx`, and the
reason is below.

## Button emphasis carries the meaning

Four tiers, and a stack card uses all four (`stack-card.tsx`):

| Tier | What it means | Variant | On the card |
| --- | --- | --- | --- |
| Solid primary | the affirmative lifecycle action | `default` | Start |
| Destructive tint | takes services offline | `destructive` | Stop |
| Outline | changes something, stack stays up | `outline` | Update, Restart |
| Ghost | only opens a view | `ghost` | Logs, Compose, `.env` |

Colour is spent, not sprinkled. Two things it is deliberately **not** spent on:

**Start is not green.** There is no `--success` token; the emerald in
`status-badge.tsx` is a raw Tailwind colour. A green Start would mean inventing
a semantic colour for one control, and it would collide — emerald *is* the
"Running" dot, so a green Start always sits beside a dot that is not green,
pairing the hue that means running with the state that is not.

**Update and Restart are not coloured.** Colour on every lifecycle action is
the Portainer/Dockge look: on fifteen rows it is sixty coloured chips, and the
status dots stop being the thing the eye finds.

## The destructive variant could not read its own label

`--destructive` was doing double duty: the tint background *and* the text on
it. At 12px `font-medium` that measured **3.99:1** at rest and **3.31:1** on
hover, against the 4.5:1 that WCAG 1.4.3 asks — and it got worse exactly under
the pointer. Every other control in the app is comfortably clear of that
(Start 5.49, outline 19.71), so Stop would have been the one illegible button,
which is the opposite of what a stop button is for.

So `--destructive-emphasis` exists in `index.css`: the destructive hue at a
strength you can read *as text on the tint*. It is not
`--destructive-foreground` — in shadcn that name means the near-white you put
on a solid fill, and reusing it here would invert the convention.

```text
light  oklch(0.5 0.2 27.325)      5.57 rest · 4.63 hover
dark   oklch(0.8 0.16 22.216)     5.98 rest · 4.97 hover
```

Check the sRGB gamut when picking such a value. The obvious light candidates at
chroma 0.22 and above clip, and a clipped colour does not render at the ratio
you computed.

The same edit takes the variant's focus ring from `border-destructive/40` to
full strength: at 40% it was **1.95:1** in dark mode against the **3.76:1**
every other variant gets from `border-ring`, so switching Stop onto this
variant would have weakened keyboard focus on the one button that must not be
hit by accident. Now 4.76 light, 6.01 dark.

Two things this deliberately left alone, both pre-existing and app-wide:
`Alert variant="destructive"` still puts `text-destructive` on the card, and
the shared `focus-visible:border-ring` measures 2.44:1 in light mode. Neither
is a button colour.

`stack-card.test.tsx` asserts that Stop carries the emphasis class and that no
other button on the card does, so a `shadcn add button` that quietly restores
the upstream variant takes the suite red rather than shipping.

## The mark

lucide's `layers`, in the app's own `--primary`. It sits in the dashboard
header and on the login card as a bare glyph with `text-primary`, like every
other icon; the tile version is only for the favicon
(`frontend/public/shimau.svg`) and the README logo.

`layers` over the more obvious `container`: shimau manages *projects*, not
individual containers, and an icon that says "one container" would contradict
the first design principle. It also survives 16px in a browser tab, which
`boxes` does not.

The tile colour is `#0069a8` — the sRGB of `oklch(0.5 0.134 242.749)`, the
`--primary` token in `frontend/src/index.css`. An SVG cannot read a CSS
variable, so that hex is a copy; if the theme's primary ever changes, convert
the new value rather than picking a blue that looks close.

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

**Never put a parent-supplied callback in an effect's dependency array.** The
dashboard passes `onOpenChange={(open) => …}` inline, so its identity changes
on every render — and the dashboard re-renders every ten seconds when the
stack query polls. Listing it in the deps of the fetch effect made both
editors silently reload the file every ten seconds, discarding unsaved edits.
Both dialogs now hold the callback in a ref refreshed by its own effect and
depend on `stack` alone;
`frontend/src/components/compose-dialog.test.tsx` re-renders the parent and
asserts one fetch, so the trap cannot come back quietly.

## Web Storage may not be there

`localStorage` is not guaranteed. A browser set to block site data **throws**
on access rather than returning null; a private window can have it disabled;
and a test environment may not provide it at all — which is how this surfaced,
when the CI Node major moved to 26 and the theme provider took the suite red
with `Cannot read properties of undefined (reading 'getItem')`.

`frontend/src/components/theme-provider.tsx` now reads and writes through
`readStoredTheme` / `writeStoredTheme`, which swallow both cases. Losing a
remembered preference is a shrug; taking the whole app down with it is not.
Anything else that reaches for `localStorage`, `sessionStorage` or `matchMedia`
owes the same treatment, and
`frontend/src/components/theme-provider.test.tsx` shows the shape of the test:
stub the global to `undefined` and to a throwing object, and assert the
component still renders.

The tempting fix — stubbing storage in `frontend/src/test/setup.ts` — is the
wrong one. It makes the suite green while leaving the crash reachable by
users.

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
