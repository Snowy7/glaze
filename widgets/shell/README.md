# widgets/shell

The page the mypaper widget host loads into its transparent WebView2 (one per monitor, served at
`https://shell.mypaper/`). It hosts one sandboxed iframe per widget instance, relays widget calls to
the host, and draws edit mode. It also builds `dist/bridge.js`, the script the host injects into every
frame to define the global `mypaper` inside widgets. The contract is `docs/widgets.md`.

```sh
pnpm install
pnpm build       # typecheck, then dist/index.html (+ assets/) and dist/bridge.js
pnpm test        # grid math (vitest)
pnpm typecheck
```

Grid math, including the no-overlap rule for moves and resizes, is in `src/grid.ts` (the rule is
documented in the comment at the top of that file).
