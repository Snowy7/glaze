import { FileCode, X } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText, pickStyleSheet, type SkinEdit, type SkinKind, type ThemeEntry } from "../lib/api";
import { AppearanceEditor } from "./AppearanceEditor";
import { SaveFooter } from "./SaveFooter";
import { Button, IconButton } from "./ui";

const EMPTY: SkinEdit = { appearance: {}, css: null };

const same = (a: SkinEdit, b: SkinEdit) => JSON.stringify(a) === JSON.stringify(b);

const CSS_HINT: Record<SkinKind, string> = {
  widgets: 'Loaded after the built-in styles. Target one widget with [data-widget="clock"].',
  bars: 'Loaded after the built-in styles. Target [data-bar="top"] or [data-bar="dock"].',
};

/**
 * Edits the `widgets` or `bars` section of one user theme: an appearance layer
 * and an optional style sheet. Saving is explicit, and the theme crate checks
 * the result before anything is written.
 */
export function SkinEditor({ theme, kind }: { theme: ThemeEntry; kind: SkinKind }) {
  const [saved, setSaved] = useState<SkinEdit | null>(null);
  const [draft, setDraft] = useState<SkinEdit>(EMPTY);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [justSaved, setJustSaved] = useState(false);

  useEffect(() => {
    let current = true;
    api.getSkin(theme.dir, kind).then(
      (skin) => {
        if (!current) return;
        setSaved(skin);
        setDraft(skin);
      },
      (e: unknown) => current && setError(errorText(e)),
    );
    return () => {
      current = false;
    };
  }, [theme.dir, kind]);

  function edit(change: Partial<SkinEdit>) {
    setJustSaved(false);
    setDraft((skin) => ({ ...skin, ...change }));
  }

  async function chooseCss() {
    const picked = await pickStyleSheet();
    if (!picked) return;
    try {
      edit({ css: await api.importSkinCss(theme.dir, kind, picked) });
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function save() {
    setSaving(true);
    setError(null);
    try {
      const skin = await api.setSkin(theme.dir, kind, draft);
      setSaved(skin);
      setDraft(skin);
      setJustSaved(true);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  }

  const dirty = saved !== null && !same(saved, draft);

  return (
    <>
      <div className="flex-1 overflow-y-auto px-6 py-5">
        <div className="rounded-xl bg-surface px-4 pt-3 pb-1 shadow-card ring-1 ring-line-strong">
          <AppearanceEditor target={kind} value={draft.appearance} onChange={(appearance) => edit({ appearance })} fallback="the built-in look" />
        </div>

        <div className="mt-4 flex items-center gap-3 rounded-xl bg-surface p-4 shadow-card ring-1 ring-line-strong">
          <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-hi text-ink-2 ring-1 ring-line-strong">
            <FileCode className="size-4" aria-hidden />
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-[13.5px] font-medium text-ink">Style sheet</div>
            <div className="selectable truncate font-mono text-xs text-ink-3" title={draft.css ?? undefined}>
              {draft.css ?? "None"}
            </div>
            <div className="mt-0.5 text-xs text-ink-3">{CSS_HINT[kind]}</div>
          </div>
          <Button size="sm" onClick={() => void chooseCss()}>
            {draft.css ? "Replace" : "Choose"}
          </Button>
          {draft.css && <IconButton icon={X} size="sm" variant="ghost" label="Remove style sheet" onClick={() => edit({ css: null })} />}
        </div>
      </div>

      <SaveFooter
        dirty={dirty}
        saving={saving}
        justSaved={justSaved}
        error={error}
        cleared={same(draft, EMPTY)}
        onClear={() => {
          setJustSaved(false);
          setDraft(EMPTY);
        }}
        onRevert={() => saved && setDraft(saved)}
        onSave={() => void save()}
      />
    </>
  );
}
