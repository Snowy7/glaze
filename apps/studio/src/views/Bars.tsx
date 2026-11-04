import { TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { BarInspector, BarsOverview, ModuleCatalog, ModuleInspector } from "../components/bars/BarsInspector";
import { zonesOf, type BarPick, type ModuleMove } from "../components/stage/BarsLayer";
import { BarsScene } from "../components/stage/BarsScene";
import { Button, EditorLayout, Inspector, InspectorHeader, InspectorSection, PageBar, SaveState, Skeleton } from "../components/ui";
import { api, errorText, type BarsCatalog, type BarsState } from "../lib/api";
import { getIn, resolve, setIn, type Path, type Resolved } from "../lib/barsDoc";
import { BarsContext, type BarsEditor } from "../lib/barsContext";
import type { ZoneId } from "../lib/barsGeometry";
import { listOf, moveModule, zoneHolding } from "../lib/barsModules";
import type { BarKind, ModuleRef } from "../lib/barsSchema";
import { useDesktop } from "../lib/desktop";
import { describeModule } from "../lib/modules";
import { useNav } from "../lib/nav";
import { useStudio } from "../lib/studio";
import { useAutosaved } from "../lib/useAutosaved";
import { useWallpaperDataUrl } from "../lib/wallpaperImage";

/** What the inspector shows: both bars, one bar, one of its modules, or the modules to add to it. */
type View =
  | { kind: "bars" }
  | { kind: "bar"; bar: BarKind }
  | { kind: "module"; bar: BarKind; zone: ZoneId; ref: ModuleRef }
  | { kind: "add"; bar: BarKind; zone: ZoneId };

const viewOf = (pick: BarPick): View => (pick.module ? { kind: "module", bar: pick.bar, zone: pick.module.zone, ref: pick.module.ref } : { kind: "bar", bar: pick.bar });

/**
 * The view as the file now stands: a module that moved is followed into its new list, and one
 * that's gone (or a bar that lost its lists) falls back to its bar.
 */
function settle(view: View, config: Resolved): View {
  if (view.kind === "bars" || view.kind === "bar") return view;
  const zones = zonesOf(config, view.bar);
  if (view.kind === "add") return zones.length > 0 ? view : { kind: "bar", bar: view.bar };
  if (zones.includes(view.zone) && listOf(config, view.bar, view.zone).includes(view.ref)) return view;
  const zone = zoneHolding(config, view.bar, view.ref, zones);
  return zone ? { ...view, zone } : { kind: "bar", bar: view.bar };
}

/**
 * The top bar and the bottom bar, live on your wallpaper: click a bar or a module to change it,
 * drag modules to move them. Edits `bars.json`, which the running bars follow as you go.
 */
export function Bars() {
  const { route } = useNav();
  const { components, notify } = useStudio();
  const desktop = useDesktop();
  const wallpaper = useWallpaperDataUrl(desktop.active?.preview ?? null);
  const [catalog, setCatalog] = useState<BarsCatalog | null>(null);
  const [view, setView] = useState<View>(route.page === "bars" && route.select ? { kind: "bar", bar: route.select } : { kind: "bars" });
  const file = useAutosaved<BarsState>({
    load: api.getBars,
    save: (state) => api.setBars(state.doc),
    concerns: (changes) => changes.bars,
    label: "Change the bars",
  });

  useEffect(() => {
    let current = true;
    api.barsCatalog().then(
      (loaded) => current && setCatalog(loaded),
      (e: unknown) => current && notify(errorText(e), "error"),
    );
    return () => {
      current = false;
    };
  }, [notify]);

  const state = file.value;
  const problem = state?.problem ?? null;
  const editor = useMemo<BarsEditor | null>(() => {
    if (!state || !catalog || state.problem) return null;
    const { doc } = state;
    return {
      doc,
      resolved: resolve(catalog, doc),
      catalog,
      set: (path: Path, value: unknown, label?: string) => file.edit((current) => ({ ...current, doc: setIn(current.doc, path, value) }), label),
      update: (change, label) => file.edit((current) => ({ ...current, doc: change(current.doc) }), label),
      has: (path: Path) => getIn(doc, path) !== undefined,
    };
  }, [state, catalog, file.edit]);

  const running = components?.find((c) => c.id === "bars")?.running;
  const shown = editor ? settle(view, editor.resolved) : view;
  const selection: BarPick | null = shown.kind === "bars" ? null : shown.kind === "module" ? { bar: shown.bar, module: { zone: shown.zone, ref: shown.ref } } : { bar: shown.bar };

  async function startOver() {
    try {
      await api.resetBars();
      await file.reload();
      setView({ kind: "bars" });
      notify("The bars are back to their defaults");
    } catch (e) {
      notify(errorText(e), "error");
    }
  }

  function move({ from, to }: ModuleMove) {
    if (!editor) return;
    const { label } = describeModule(from.ref);
    editor.update((doc) => moveModule(doc, editor.resolved, from, to), to ? `Move ${label}` : `Remove ${label}`);
  }

  // A file the bars can't read is never rewritten here: the way on is fixing it by hand, or starting over.
  const trouble = file.error ? `Your bar settings can't be read: ${file.error}` : problem ? `The bars can't use your settings file as it is: ${problem}. Fix it in a text editor, or start over.` : null;

  const toBar = (bar: BarKind) => setView({ kind: "bar", bar });
  const inspector = (() => {
    if (!editor) return trouble ? <Trouble message={trouble} onStartOver={() => void startOver()} /> : <InspectorLoading />;
    switch (shown.kind) {
      case "bars":
        return <BarsOverview onPick={(pick) => setView(viewOf(pick))} />;
      case "bar":
        return <BarInspector key={shown.bar} bar={shown.bar} wallpaper={wallpaper} onPick={(pick) => setView(viewOf(pick))} onAdd={(zone) => setView({ kind: "add", bar: shown.bar, zone })} onBack={() => setView({ kind: "bars" })} />;
      case "module":
        return <ModuleInspector key={`${shown.bar}:${shown.ref}`} bar={shown.bar} zone={shown.zone} moduleRef={shown.ref} onBack={() => toBar(shown.bar)} />;
      case "add":
        return <ModuleCatalog key={shown.bar} bar={shown.bar} zone={shown.zone} onAdded={(zone, ref) => setView({ kind: "module", bar: shown.bar, zone, ref })} onBack={() => toBar(shown.bar)} />;
    }
  })();

  return (
    <>
      <PageBar title="Bars" detail={state && running === false && !problem ? "Not running: your changes apply when the bars start" : undefined}>
        <SaveState saving={file.saving} />
      </PageBar>
      <EditorLayout
        stage={<BarsScene desktop={desktop} editor={editor} wallpaper={wallpaper} selection={selection} onSelect={(pick) => setView(pick ? viewOf(pick) : { kind: "bars" })} onMove={move} />}
        inspector={
          <Inspector label="Inspector">
            {editor ? <BarsContext value={editor}>{inspector}</BarsContext> : inspector}
          </Inspector>
        }
      />
    </>
  );
}

/** A bars file Studio won't change, and the way out. */
function Trouble({ message, onStartOver }: { message: string; onStartOver: () => void }) {
  return (
    <>
      <InspectorHeader icon={TriangleAlert} title="Bars" detail="Your settings need fixing" />
      <InspectorSection>
        <p role="alert" className="selectable text-[13px] leading-snug text-ink">
          {message}
        </p>
        <Button size="sm" className="self-start" onClick={onStartOver}>
          Start over
        </Button>
      </InspectorSection>
    </>
  );
}

function InspectorLoading() {
  return (
    <div className="flex flex-col gap-3 px-4 py-4" aria-busy>
      <Skeleton className="h-9 rounded-lg" />
      <Skeleton className="h-24 rounded-lg" />
      <Skeleton className="h-40 rounded-lg" />
    </div>
  );
}
