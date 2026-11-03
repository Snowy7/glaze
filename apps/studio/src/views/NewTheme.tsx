import { File, FolderOpen } from "lucide-react";
import { useId, useState } from "react";

import type { View } from "../components/Sidebar";
import { Button, PageHeader } from "../components/ui";
import { api, errorText, pickFile, pickFolder, type Fit, type SourceInfo } from "../lib/api";
import { KINDS } from "../lib/kinds";
import { useStudio } from "../lib/studio";

const FITS: { fit: Fit; label: string; hint: string }[] = [
  { fit: "cover", label: "Cover", hint: "Fill the screen, cropping the overflow" },
  { fit: "contain", label: "Contain", hint: "Show everything, letterboxed" },
  { fit: "stretch", label: "Stretch", hint: "Fill the screen, ignoring aspect ratio" },
  { fit: "center", label: "Center", hint: "Pixel for pixel, centered" },
];

export function NewTheme({ onNavigate }: { onNavigate: (view: View) => void }) {
  const { createTheme, busy } = useStudio();
  const [source, setSource] = useState<SourceInfo | null>(null);
  const [pickError, setPickError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [fit, setFit] = useState<Fit>("cover");
  const [apply, setApply] = useState(true);
  const nameId = useId();

  async function choose(picker: () => Promise<string | null>) {
    try {
      const path = await picker();
      if (!path) return;
      const info = await api.inspectSource(path);
      setSource(info);
      setName(info.suggestedName);
      setPickError(null);
    } catch (error) {
      setSource(null);
      setPickError(errorText(error));
    }
  }

  async function create() {
    if (!source) return;
    const created = await createTheme({
      source: source.path,
      name,
      fit: source.usesFit ? fit : null,
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
      <PageHeader title="New theme" subtitle="Turn a picture, video, folder, shader or 3D scene into a theme." />

      <section aria-labelledby="source-heading" className="max-w-2xl">
        <h2 id="source-heading" className="mb-3 text-xs font-semibold tracking-[0.12em] text-ink-3 uppercase">
          1 · Source
        </h2>
        <div className="grid grid-cols-2 gap-4">
          <PickCard icon={File} title="Choose a file" detail="Image, video, .wgsl shader, .glb / .gltf scene" onClick={() => void choose(pickFile)} />
          <PickCard icon={FolderOpen} title="Choose a folder" detail="Pictures in the folder become a slideshow" onClick={() => void choose(pickFolder)} />
        </div>
        {pickError && (
          <p role="alert" className="selectable mt-3 text-[13px] text-danger">
            {pickError}
          </p>
        )}
      </section>

      {source && kind && (
        <section aria-labelledby="details-heading" className="mt-9 max-w-2xl animate-[rise_300ms_ease-out_backwards]">
          <h2 id="details-heading" className="mb-3 text-xs font-semibold tracking-[0.12em] text-ink-3 uppercase">
            2 · Details
          </h2>
          <div className="rounded-2xl bg-surface p-5 shadow-card ring-1 ring-line">
            <div className="flex items-center gap-3">
              <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-surface-hi text-accent">
                <kind.icon className="size-5" aria-hidden />
              </span>
              <div className="min-w-0">
                <div className="font-display text-[15px] font-semibold">{kind.label} theme</div>
                <div className="selectable truncate font-mono text-xs text-ink-3" title={source.path}>
                  {source.path}
                </div>
              </div>
            </div>

            <label htmlFor={nameId} className="mt-5 mb-1.5 block text-[13px] font-medium text-ink-2">
              Name
            </label>
            <input
              id={nameId}
              value={name}
              onChange={(e) => setName(e.target.value)}
              maxLength={60}
              className="selectable h-10 w-full rounded-lg bg-canvas px-3 text-[14px] ring-1 ring-line-strong transition placeholder:text-ink-3 hover:ring-ink-3 focus:ring-2 focus:ring-accent focus:outline-none"
            />

            {source.usesFit && (
              <fieldset className="mt-5">
                <legend className="mb-1.5 text-[13px] font-medium text-ink-2">Fit</legend>
                <div className="grid grid-cols-4 gap-1 rounded-xl bg-canvas p-1 ring-1 ring-line">
                  {FITS.map((option) => (
                    <label
                      key={option.fit}
                      title={option.hint}
                      className="relative cursor-default rounded-lg py-2 text-center text-[13px] font-medium text-ink-2 transition-colors has-checked:bg-surface-hi has-checked:text-ink has-checked:shadow-sm has-focus-visible:outline-2 has-focus-visible:outline-accent hover:text-ink"
                    >
                      <input
                        type="radio"
                        name="fit"
                        value={option.fit}
                        checked={fit === option.fit}
                        onChange={() => setFit(option.fit)}
                        className="sr-only"
                      />
                      {option.label}
                    </label>
                  ))}
                </div>
                <p className="mt-1.5 text-xs text-ink-3">{FITS.find((o) => o.fit === fit)?.hint}</p>
              </fieldset>
            )}

            <label className="mt-5 flex cursor-default items-center gap-3 text-[13.5px]">
              <input type="checkbox" checked={apply} onChange={(e) => setApply(e.target.checked)} className="size-4 accent-[var(--accent)]" />
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

function PickCard({ icon: Icon, title, detail, onClick }: { icon: typeof File; title: string; detail: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group flex cursor-default flex-col items-start gap-3 rounded-2xl border border-dashed border-line-strong bg-surface/50 p-5 text-left transition duration-200 hover:border-accent hover:bg-surface"
    >
      <span className="grid size-10 place-items-center rounded-xl bg-surface-hi text-ink-2 transition-colors group-hover:text-accent">
        <Icon className="size-5" aria-hidden />
      </span>
      <span>
        <span className="block font-display text-[15px] font-semibold">{title}</span>
        <span className="mt-0.5 block text-[12.5px] text-ink-3">{detail}</span>
      </span>
    </button>
  );
}
