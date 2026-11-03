# widgets/shell

The page the mypaper widget host loads into its transparent WebView2 (one per monitor, served at
`https://shell.mypaper/`). It hosts one sandboxed iframe per widget instance, answers the widget
SDK's handshake, checks permissions and relays calls to the host, and draws edit mode. It also builds
`dist/harden.js`, a tiny script the host injects into every frame to remove WebRTC. The contract is
`docs/widgets.md`; the widget side of the wire is `packages/widget-sdk`.

```sh
pnpm install
pnpm build       # typecheck, then dist/index.html (+ assets/) and dist/harden.js
pnpm test        # grid math and permission rules (vitest)
pnpm typecheck
```

Grid math, including the no-overlap rule for moves and resizes, is in `src/grid.ts` (the rule is
documented in the comment at the top of that file). The permission rule is `src/permissions.ts`.
