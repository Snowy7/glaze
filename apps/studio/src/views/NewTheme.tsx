import { AppWindow, File, FolderOpen, Globe, TriangleAlert, type LucideIcon } from "lucide-react";
import { useId, useState, type FormEvent } from "react";

import type { View } from "../components/Sidebar";
import { Button, INPUT_CLASS, PageHeader, SectionLabel, Segmented } from "../components/ui";
import { api, errorText, pickFolder, pickHtml, pickMedia, pickProgram, type Fit, type SourceInfo } from "../lib/api";
import { KINDS } from "../lib/kinds";
import { useStudio } from "../lib/studio";

const FITS: { value: Fit; label: string; hint: string }[] = [
  { value: "cover", label: "Cover", hint: "Fill the screen, cropping the overflow" },
  { value: "contain", label: "Contain", hint: "Show everything, letterboxed" },
  { value: "stretch", label: "Stretch", hint: "Fill the screen, ignoring aspect ratio" },
  { value: "center", label: "Center", hint: "Pixel for pixel, centered" },
];

export function NewTheme({ onNavigate }: { onNavigate: (view: View) => void }) {
  const { createTheme, busy } = useStudio();
  const [source, setSource] = useState<SourceInfo | null>(null);
  const [pickError, setPickError] = useState<string | null>(null);
  const [webOpen, setWebOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [fit, setFit] = useState<Fit>("cover");
  const [args, setArgs] = useState("");
  const [apply, setApply] = useState(true);
  const nameId = useId();
  const urlId = useId();
  const argsId = useId();

  /** Runs `inspect` on a picked path or typed address and shows the details form for it. */
  async function choose(find: () => Promise<SourceInfo | null>) {
    try {
      const info = await find();
      if (!info) return;
      setSource(info);
      setName(info.suggestedName);
      setArgs(info.defaultArgs);
      setPickError(null);
    } catch (error) {
      setSource(null);
      setPickError(errorText(error));
    }
  }

  const picked = (picker: () => Promise<string | null>) => async () => {
    const path = await picker();
    return path ? api.inspectSource(path) : null;
  };

  function submitUrl(event: FormEvent) {
    event.preventDefault();
    void choose(() => api.inspectUrl(url));
  }

  async function create() {
    if (!source) return;
    const created = await createTheme({
      source: source.source,
      name,
      fit: source.usesFit ? fit : null,
      args: source.kind === "app" ? args : null,
      apply,
    });
    if (created) {
      setSource(null);
      onNavigate("gallery");
    }
  }

  const kind = source ? KINDS[source.kind] : null;

  return (
    <>
      <PageHeader title="New theme" subtitle="Turn a picture, video, folder, shader, 3D scene, web page or program into a theme." />

      <section aria-labelledby="source-heading" className="max-w-3xl">
        <SectionLabel id="source-heading">1 · Source</SectionLabel>
        <div className="grid grid-cols-2 gap-4">
          <PickCard icon={File} title="Choose a file" detail="Image, video, .wgsl shader, .glb / .gltf scene" onClick={() => void choose(picked(pickMedia))} />
          <PickCard icon={FolderOpen} title="Choose a folder" detail="Pictures in the folder become a slideshow" onClick={() => void choose(picked(pickFolder))} />
          <PickCard icon={Globe} title="Web page" detail="A website or local .html file, drawn by WebView2" selected={webOpen} onClick={() => setWebOpen((open) => !open)} />
          <PickCard icon={AppWindow} title="Program" detail=".exe that renders into the desktop, like a Unity player" onClick={() => void choose(picked(pickProgram))} />
        </div>

        {webOpen && (
          <form onSubmit={submitUrl} className="mt-4 animate-fade-in rounded-2xl bg-surface p-4 ring-1 ring-line">
            <label htmlFor={urlId} className="mb-1.5 block text-[13px] font-medium text-ink-2">
              Web address
            </label>
            <div className="flex gap-2">
              <input
                id={urlId}
                type="url"
                inputMode="url"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder="https://example.com"
                spellCheck={false}
                autoFocus
                className={INPUT_CLASS}
              />
              <Button type="submit" variant="primary" disabled={url.trim() === ""}>
                Use address
              </Button>
            </div>
            <p className="mt-2 text-xs text-ink-3">
              Or{" "}
              <button type="button" onClick={() => void choose(picked(pickHtml))} className="cursor-default text-ink underline underline-offset-2 hover:text-ink-2">
                choose an .html file
              </button>
              .
            </p>
          </form>
        )}

        {pickError && (
          <p role="alert" className="selectable mt-3 flex items-start gap-2 text-[13px] text-ink">
            <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden />
            {pickError}
          </p>
        )}
      </section>

      {source && kind && (
        <section aria-labelledby="details-heading" className="mt-9 max-w-3xl animate-rise">
          <SectionLabel id="details-heading">2 · Details</SectionLabel>
          <div className="rounded-2xl bg-surface p-5 shadow-card ring-1 ring-line">
            <div className="flex items-center gap-3">
              <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-solid text-solid-ink">
                <kind.icon className="size-5" aria-hidden />
              </span>
              <div className="min-w-0">
                <div className="font-display text-[15px] font-semibold text-ink">{kind.label} theme</div>
                <div className="selectable truncate font-mono text-xs text-ink-3" title={source.source}>
                  {source.source}
                </div>
              </div>
            </div>

            <label htmlFor={nameId} className="mt-5 mb-1.5 block text-[13px] font-medium text-ink-2">
              Name
            </label>
            <input id={nameId} value={name} onChange={(e) => setName(e.target.value)} maxLength={60} className={INPUT_CLASS} />

            {source.usesFit && (
              <>
                <Segmented<Fit> label="Fit" className="mt-5" value={fit} onChange={setFit} options={FITS} />
                <p className="mt-1.5 text-xs text-ink-3">{FITS.find((o) => o.value === fit)?.hint}</p>
              </>
            )}

            {source.kind === "app" && (
              <div className="mt-5">
                <label htmlFor={argsId} className="mb-1.5 block text-[13px] font-medium text-ink-2">
                  Arguments
                </label>
                <input
                  id={argsId}
                  value={args}
                  onChange={(e) => setArgs(e.target.value)}
                  placeholder="-parentHWND {hwnd} delayed"
                  spellCheck={false}
                  className={`${INPUT_CLASS} font-mono text-[13px]`}
                />
                <div className="mt-2 flex flex-wrap items-center gap-2">
                  <span className="text-xs text-ink-3">Presets</span>
                  {source.presets.map((preset) => (
                    <button
                      key={preset.id}
                      type="button"
                      aria-pressed={args === preset.args}
                      onClick={() => setArgs(preset.args)}
                      className="cursor-default rounded-full px-3 py-1 text-xs font-medium text-ink-2 ring-1 ring-line-strong transition hover:ring-line-hi aria-pressed:bg-solid aria-pressed:text-solid-ink aria-pressed:ring-transparent"
                    >
                      {preset.label}
                    </button>
                  ))}
                  <button type="button" onClick={() => setArgs("")} className="cursor-default rounded-full px-3 py-1 text-xs font-medium text-ink-3 hover:text-ink">
                    Clear
                  </button>
                </div>
                <p className="mt-2 text-xs text-ink-3">
                  <code className="font-mono text-ink-2">{"{hwnd}"}</code> becomes the handle of the desktop window the program should draw into. Use quotes around values with spaces.
                </p>
              </div>
            )}

            <label className="mt-5 flex cursor-default items-center gap-3 text-[13.5px] text-ink-2">
              <input type="checkbox" checked={apply} onChange={(e) => setApply(e.target.checked)} className="size-4 accent-[var(--solid)]" />
              Apply it right away
            </label>

            <div className="mt-6 flex justify-end gap-2">
              <Button variant="ghost" onClick={() => setSource(null)}>
                Cancel
              </Button>
              <Button variant="primary" busy={busy.has("create")} disabled={name.trim() === ""} onClick={() => void create()}>
                Create theme
              </Button>
            </div>
          </div>
        </section>
      )}
    </>
  );
}

function PickCard({
  icon: Icon,
  title,
  detail,
  selected,
  onClick,
}: {
  icon: LucideIcon;
  title: string;
  detail: string;
  /** For cards that open an inline form rather than a native picker. */
  selected?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-expanded={selected}
      className="group flex cursor-default flex-col items-start gap-3 rounded-2xl border border-dashed border-line-strong bg-surface/50 p-5 text-left transition duration-200 hover:border-line-hi hover:bg-surface aria-expanded:border-solid aria-expanded:border-line-hi aria-expanded:bg-surface"
    >
      <span className="grid size-10 place-items-center rounded-xl bg-surface-hi text-ink-2 transition-colors group-hover:bg-solid group-hover:text-solid-ink group-aria-expanded:bg-solid group-aria-expanded:text-solid-ink">
        <Icon className="size-5" aria-hidden />
      </span>
      <span>
        <span className="block font-display text-[15px] font-semibold text-ink">{title}</span>
        <span className="mt-0.5 block text-[12.5px] text-ink-3">{detail}</span>
      </span>
    </button>
  );
}
