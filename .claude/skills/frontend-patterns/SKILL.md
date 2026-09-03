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

Two more are in `button.tsx` and `alert.tsx`, both moving red text onto
`--destructive-emphasis`; the reason is below. Four exceptions is enough — the
next one wants a call-site `className` first.

## Button emphasis carries the meaning

Four tiers, and a stack card uses all four (`stack-card.tsx`):

| Tier | What it means | Variant | On the card |
| --- | --- | --- | --- |
| Solid primary | the affirmative lifecycle action | `default` | Start |
| Destructive tint | takes services offline | `destructive` | Stop |
| Outline | changes something, stack stays up | `outline` | Update, Restart |
| Ghost | only opens a view | `ghost` | Logs, Compose, `.env`, Show output |

Colour is spent, not sprinkled. Two things it is deliberately **not** spent on:

**Start is not green.** There is no `--success` token; the emerald in
`status-badge.tsx` is a raw Tailwind colour. A green Start would mean inventing
a semantic colour for one control, and it would collide — emerald *is* the
"Running" dot, so a green Start always sits beside a dot that is not green,
pairing the hue that means running with the state that is not.

**Update and Restart are not coloured.** Colour on every lifecycle action is
the Portainer/Dockge look: on fifteen rows it is sixty coloured chips, and the
status dots stop being the thing the eye finds.

## `npm run typecheck` is `tsc -b`, and it has to stay that way

`tsconfig.json` is a solution file: `"files": []` and two project references.
`tsc --noEmit` against it therefore compiles **nothing** and exits 0 whatever
the code says — the script read that way for a while, and the CI step named
"Type check" was proving nothing. Only `-b` walks the references; both projects
already set `noEmit`, so build mode never writes anything either.

The tell was `npm run build` failing on an error `npm run typecheck` had just
passed: `tsc -b && vite build` was the only thing checking types. If you touch
the scripts, check the gate can still fail — write a deliberate type error and
watch it turn red.

Under `erasableSyntaxOnly`, which both projects set, TypeScript-only runtime
syntax is out: no constructor parameter properties, no `enum`, no namespaces.
Fields get declared and assigned.

## The page runs under a Content-Security-Policy

`CONTENT_SECURITY_POLICY` in `backend/src/api/mod.rs` ships on every response,
and `script-src 'self'` is the half worth keeping. What it forbids, in the
order you are likely to trip over it:

- **No third-party origin, for anything.** A Google Fonts `<link>`, an
  analytics snippet, a CDN `<script>` — all refused, and refused *silently* as
  far as the UI is concerned. Fonts are already self-hosted through the Geist
  package and bundled into `/assets`; keep it that way.
- **No inline `<script>` and no `eval`.** Vite emits neither today. A build
  option that starts inlining the module-preload polyfill would, so a build
  change is a policy change.
- **`style-src` keeps `'unsafe-inline'`**, and has to: Radix positions floating
  elements with inline `style` attributes and CodeMirror injects its theme as a
  `<style>` element. Neither can be nonced from a static file server.
- **`img-src` allows `data:`** for the SVG marks Tailwind inlines. A `blob:`
  image — a generated download, say — would need the directive widened.

Verified in Chromium against a running instance, not by reading the policy:
login, both editors (typing and saving included), the `.env` reveal, both SSE
streams, the theme toggle and a sign-out, watching `securitypolicyviolation`.
Zero violations, and an injected inline script refused. If you change the
policy or add a third-party asset, walk that path again — jsdom does not
enforce CSP, so the Vitest suite will never tell you.

## Contrast is a gate, not a habit

`scripts/check-contrast.mjs` reads the tokens out of `index.css`, composites
the tints the UI actually renders, and fails when a pair drops below what its
role requires — 4.5:1 for text, 3:1 for a focus indicator. CI runs it as
**Theme contrast**. `node scripts/check-contrast.mjs --report` prints every
pair with its ratio, which is the fastest way to tune a token.

Move a token and the gate tells you what you broke, so the ratios are not
repeated here. What the gate cannot tell you:

**Composite alpha in gamma-encoded sRGB, not linear light.** A browser blends a
translucent layer against the encoded value behind it. Getting this wrong moved
the tinted pairs by about a point and a half — enough to invent failures that
are not there, or hide real ones. The script does it correctly; a calculation
done any other way has to match it.

**Check the sRGB gamut.** The obvious light red candidates at chroma 0.22 and
above clip, and a clipped colour does not render at the ratio you computed.

**A tint background is the trap.** `--destructive` used to do double duty as
the tint *and* the text on it, which is how the Stop button ended up below 4:1
while the same red on a plain card was comfortably fine. Hence
`--destructive-emphasis`: the destructive hue at a strength readable as text.
It is not `--destructive-foreground` — in shadcn that name means the near-white
you put on a solid fill, and reusing it would invert the convention. Every red
text in the app uses the emphasis token, so there is one red for one meaning.

**Contrast is not the whole of a focus ring.** The destructive variant's ring
was `border-destructive/40` and the shared `--ring` was light enough to fail
1.4.11 outright; both are at full strength now. `--ring` shows only on
`:focus-visible`, so a darker value costs mouse users nothing and buys keyboard
users the indicator the spec asks for.

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
