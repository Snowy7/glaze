import { AppWindow, Check, File, FolderOpen, Globe, TriangleAlert, type LucideIcon } from "lucide-react";
import { useId, useState, type FormEvent } from "react";

import type { View } from "../components/Sidebar";
import { KindBadge, ThemePreview } from "../components/ThemeCard";
import { Badge, Button, Card, Field, Page, Switch, TextField, TextInput } from "../components/ui";
import { api, errorText, pickFolder, pickHtml, pickMedia, pickProgram, type Fit, type Kind, type SourceInfo } from "../lib/api";
import { KINDS } from "../lib/kinds";
import { useStudio } from "../lib/studio";

const FITS: { value: Fit; label: string; hint: string }[] = [
  { value: "cover", label: "Cover", hint: "Fill the screen, cropping the overflow" },
  { value: "contain", label: "Contain", hint: "Show everything, letterboxed" },
  { value: "stretch", label: "Stretch", hint: "Fill the screen, ignoring aspect ratio" },
  { value: "center", label: "Center", hint: "Pixel for pixel, centered" },
];

/** Where the source came from: which card to mark. */
type Via = "file" | "folder" | "web" | "app";

export function NewTheme({ onNavigate }: { onNavigate: (view: View) => void }) {
  const { createTheme, busy, roots } = useStudio();
  const [source, setSource] = useState<SourceInfo | null>(null);
  const [via, setVia] = useState<Via | null>(null);
  const [pickError, setPickError] = useState<string | null>(null);
  const [webOpen, setWebOpen] = useState(false);
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [fit, setFit] = useState<Fit>("cover");
  const [args, setArgs] = useState("");
  const [apply, setApply] = useState(true);
  const applyId = useId();

  /** Runs `inspect` on a picked path or typed address and shows the details form for it. */
  async function choose(from: Via, find: () => Promise<SourceInfo | null>) {
    try {
      const info = await find();
      if (!info) return;
      setSource(info);
      setVia(from);
      setName(info.suggestedName);
      setArgs(info.defaultArgs);
      setPickError(null);
    } catch (error) {
      setSource(null);
      setVia(null);
      setPickError(errorText(error));
    }
  }

  const picked = (picker: () => Promise<string | null>) => async () => {
    const path = await picker();
    return path ? api.inspectSource(path) : null;
  };

  function submitUrl(event: FormEvent) {
    event.preventDefault();
    void choose("web", () => api.inspectUrl(url));
  }

  function reset() {
    setSource(null);
    setVia(null);
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
      reset();
      onNavigate("gallery");
    }
  }

  const kind = source ? KINDS[source.kind] : null;

  return (
    <Page title="New theme" subtitle="Turn a picture, video, folder, shader, 3D scene, web page or program into a theme." width="wide">
      <div className="@container flex flex-col gap-9">
        <section aria-labelledby="source-heading">
          <Step n={1} id="source-heading" title="Source" done={source !== null} />
          <div className="grid gap-3 @[30rem]:grid-cols-2 @[84rem]:grid-cols-4">
            <PickCard icon={File} title="Choose a file" detail="Image, video, .wgsl shader, .glb / .gltf scene" chosen={via === "file"} onClick={() => void choose("file", picked(pickMedia))} />
            <PickCard icon={FolderOpen} title="Choose a folder" detail="Pictures in the folder become a slideshow" chosen={via === "folder"} onClick={() => void choose("folder", picked(pickFolder))} />
            <PickCard icon={Globe} title="Web page" detail="A website or local .html file, drawn by WebView2" chosen={via === "web"} expanded={webOpen} onClick={() => setWebOpen((open) => !open)} />
            <PickCard icon={AppWindow} title="Program" detail=".exe that renders into the desktop, like a Unity player" chosen={via === "app"} onClick={() => void choose("app", picked(pickProgram))} />
          </div>

          {webOpen && (
            <form onSubmit={submitUrl} className="mt-3 max-w-2xl animate-fade-in rounded-xl bg-surface p-4 ring-1 ring-line-strong">
              <Field label="Web address">
                <div className="flex gap-2">
                  <TextInput type="url" inputMode="url" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://example.com" spellCheck={false} autoFocus className="flex-1" />
                  <Button type="submit" variant="primary" disabled={url.trim() === ""}>
                    Use address
                  </Button>
                </div>
              </Field>
              <p className="mt-2 text-xs text-ink-3">
                Or{" "}
                <button type="button" onClick={() => void choose("web", picked(pickHtml))} className="cursor-default text-ink underline decoration-line-hi underline-offset-2 hover:decoration-ink">
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

        <div className="grid items-start gap-x-10 gap-y-8 @[58rem]:grid-cols-[minmax(0,1fr)_20rem] @[96rem]:grid-cols-[minmax(0,1fr)_24rem]">
          <section aria-labelledby="details-heading" className={`min-w-0 ${source ? "animate-rise" : ""}`}>
            <Step n={2} id="details-heading" title="Details" done={false} muted={!source} />
            {source && kind ? (
              <Card flush>
                <div className="flex items-center gap-3 border-b border-line px-5 py-4">
                  <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-solid text-solid-ink">
                    <kind.icon className="size-5" aria-hidden />
                  </span>
                  <div className="min-w-0">
                    <div className="text-[14px] font-semibold text-ink">{kind.label} theme</div>
                    <div className="selectable truncate font-mono text-xs text-ink-3" title={source.source}>
                      {source.source}
                    </div>
                  </div>
                </div>

                <div className="@container flex flex-col gap-6 px-5 py-5">
                  <div className="grid items-end gap-x-6 gap-y-6 @[46rem]:grid-cols-2">
                    <TextField label="Name" value={name} onChange={(e) => setName(e.target.value)} maxLength={60} />
                    <div className="flex h-fit items-center gap-3 rounded-lg bg-canvas px-3.5 py-2.5 ring-1 ring-line ring-inset">
                      <div className="min-w-0 flex-1">
                        <label htmlFor={applyId} className="block text-[13.5px] font-medium text-ink">
                          Apply it right away
                        </label>
                        <p className="text-[12px] text-ink-3">Your desktop switches to it as soon as it's created.</p>
                      </div>
                      <Switch id={applyId} label="Apply it right away" checked={apply} onChange={setApply} />
                    </div>
                  </div>

                  {source.usesFit && <FitPicker value={fit} onChange={setFit} />}

                  {source.kind === "app" && (
                    <div>
                      <TextField
                        label="Arguments"
                        mono
                        value={args}
                        onChange={(e) => setArgs(e.target.value)}
                        placeholder="-parentHWND {hwnd} delayed"
                        description={
                          <>
                            <code className="font-mono text-ink-2">{"{hwnd}"}</code> becomes the handle of the desktop window the program should draw into. Use quotes around values with spaces.
                          </>
                        }
                      />
                      <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
                        <span className="mr-1 text-xs text-ink-3">Presets</span>
                        {source.presets.map((preset) => (
                          <button
                            key={preset.id}
                            type="button"
                            aria-pressed={args === preset.args}
                            onClick={() => setArgs(preset.args)}
                            className="h-7 cursor-default rounded-md px-2.5 text-xs font-medium text-ink-2 ring-1 ring-line-strong transition ring-inset hover:bg-surface-hi hover:text-ink aria-pressed:bg-surface-top aria-pressed:text-ink aria-pressed:ring-line-hi"
                          >
                            {preset.label}
                          </button>
                        ))}
                        <Button size="sm" variant="ghost" onClick={() => setArgs("")}>
                          Clear
                        </Button>
                      </div>
                    </div>
                  )}
                </div>

                <div className="flex justify-end gap-2 border-t border-line bg-canvas/40 px-5 py-3.5">
                  <Button variant="ghost" onClick={reset}>
                    Cancel
                  </Button>
                  <Button variant="primary" busy={busy.has("create")} disabled={name.trim() === ""} onClick={() => void create()}>
                    {apply ? "Create and apply" : "Create theme"}
                  </Button>
                </div>
              </Card>
            ) : (
              <p className="rounded-xl border border-dashed border-line-strong px-5 py-6 text-[13px] text-ink-3">Pick a source above, and its name and options show here.</p>
            )}
          </section>

          <aside aria-label="Preview" className="@[58rem]:sticky @[58rem]:top-[calc(var(--header-h)+1.5rem)]">
            {source ? <ResultPreview name={name} kind={source.kind} apply={apply} folder={roots[0]} /> : <SupportedKinds />}
          </aside>
        </div>
      </div>
    </Page>
  );
}

function Step({ n, id, title, done, muted = false }: { n: number; id: string; title: string; done: boolean; muted?: boolean }) {
  return (
    <h2 id={id} className={`mb-3 flex items-center gap-2.5 text-[14px] font-semibold ${muted ? "text-ink-3" : "text-ink"}`}>
      <span
        className={`grid size-6 place-items-center rounded-full text-[12px] font-semibold tabular-nums transition-colors ${
          done ? "bg-solid text-solid-ink" : muted ? "text-ink-3 ring-1 ring-line-strong ring-inset" : "bg-surface-top text-ink ring-1 ring-line-hi ring-inset"
        }`}
      >
        {done ? <Check className="size-3.5" strokeWidth={2.5} aria-hidden /> : n}
      </span>
      {title}
    </h2>
  );
}

function PickCard({
  icon: Icon,
  title,
  detail,
  chosen,
  expanded,
  onClick,
}: {
  icon: LucideIcon;
  title: string;
  detail: string;
  /** The current source came from here. */
  chosen: boolean;
  /** For cards that open an inline form rather than a native picker. */
  expanded?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-expanded={expanded}
      className={`group flex cursor-default items-start gap-3.5 rounded-xl p-4 text-left transition duration-150 active:scale-[0.99] @[84rem]:p-5 ${
        chosen ? "bg-surface-hi ring-2 ring-ink" : "bg-surface ring-1 ring-line-strong hover:bg-surface-hi hover:ring-line-hi aria-expanded:bg-surface-hi aria-expanded:ring-line-hi"
      }`}
    >
      <span
        className={`grid size-10 shrink-0 place-items-center rounded-lg transition-colors ${
          chosen ? "bg-solid text-solid-ink" : "bg-surface-hi text-ink-2 ring-1 ring-line-strong group-hover:bg-surface-top group-hover:text-ink"
        }`}
      >
        <Icon className="size-5" strokeWidth={1.7} aria-hidden />
      </span>
      <span className="min-w-0">
        <span className="block text-[14px] font-semibold text-ink">{title}</span>
        <span className="mt-0.5 block text-[12.5px] leading-snug text-ink-3">{detail}</span>
      </span>
    </button>
  );
}

/** A tiny landscape picture, so each fit can show what it does to one. */
function Picture({ fit }: { fit: Fit }) {
  const aspect = { cover: "xMidYMid slice", contain: "xMidYMid meet", stretch: "none", center: "xMidYMid meet" }[fit];
  return (
    <svg viewBox="0 0 40 30" preserveAspectRatio={aspect} className={fit === "center" ? "h-1/2 w-1/2" : "size-full"} aria-hidden>
      <rect width="40" height="30" fill="#8a8a8a" />
      <circle cx="29" cy="9" r="4" fill="#e6e6e6" />
      <path d="M0 30 L13 12 L21 22 L27 16 L40 30 Z" fill="#3d3d3d" />
    </svg>
  );
}

/** How a picture fills the screen: four tiles, each showing it. A radio group, so arrow keys move the choice. */
function FitPicker({ value, onChange }: { value: Fit; onChange: (fit: Fit) => void }) {
  const name = useId();
  return (
    <fieldset>
      <legend className="mb-1.5 text-[13px] font-medium text-ink">Fit</legend>
      <div className="grid max-w-4xl grid-cols-2 gap-2 @[34rem]:grid-cols-4">
        {FITS.map((fit) => (
          <label
            key={fit.value}
            title={fit.hint}
            className="group cursor-default rounded-lg bg-canvas p-1.5 ring-1 ring-line-strong transition ring-inset hover:bg-surface-hi hover:ring-line-hi has-checked:bg-surface-top has-checked:ring-2 has-checked:ring-ink has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-ink"
          >
            <input type="radio" name={name} value={fit.value} checked={value === fit.value} onChange={() => onChange(fit.value)} className="sr-only" />
            <span className="grid aspect-[16/10] place-items-center overflow-hidden rounded bg-[#1d1d1d] ring-1 ring-white/10">
              <Picture fit={fit.value} />
            </span>
            <span className="block px-0.5 pt-1.5 text-[12.5px] font-medium text-ink-2 group-has-checked:text-ink">{fit.label}</span>
          </label>
        ))}
      </div>
      <p className="mt-2 text-xs text-ink-3">{FITS.find((o) => o.value === value)?.hint}</p>
    </fieldset>
  );
}

/** The theme as it will show in the gallery, updating as the name is typed. */
function ResultPreview({ name, kind, apply, folder }: { name: string; kind: Kind; apply: boolean; folder: string | undefined }) {
  const title = name.trim() || "Untitled";
  return (
    <div>
      <p className="mb-2.5 px-0.5 text-[12.5px] font-medium text-ink-3">In the gallery</p>
      <div className="rounded-xl bg-surface p-1.5 shadow-card ring-1 ring-line-strong">
        <div className="relative aspect-[16/10] overflow-hidden rounded-lg">
          <ThemePreview name={title} kind={kind} preview={null} />
          <div className="absolute inset-x-0 top-0 flex justify-between p-2">
            <KindBadge kind={kind} />
            {apply && (
              <Badge tone="solid">
                <span className="size-1.5 rounded-full bg-current" aria-hidden />
                Active
              </Badge>
            )}
          </div>
        </div>
        <div className="px-2 pt-2.5 pb-1.5">
          <div className="truncate text-[14px] font-semibold text-ink">{title}</div>
          <div className="text-[12px] text-ink-3">Yours</div>
        </div>
      </div>
      <p className="mt-3 px-0.5 text-[12px] leading-relaxed text-ink-3">
        The picture comes from the source once it's created. It's saved in <span className="selectable font-mono text-[11.5px] break-all text-ink-2">{folder ?? "your themes folder"}</span>.
      </p>
    </div>
  );
}

/** Before a pick: what can become a theme. */
function SupportedKinds() {
  return (
    <div>
      <p className="mb-2.5 px-0.5 text-[12.5px] font-medium text-ink-3">What can be a theme</p>
      <ul className="divide-y divide-line rounded-xl bg-surface ring-1 ring-line-strong">
        {Object.values(KINDS).map(({ label, icon: Icon, hint }) => (
          <li key={label} className="flex items-start gap-3 px-3.5 py-2.5">
            <Icon className="mt-0.5 size-4 shrink-0 text-ink-2" aria-hidden />
            <div className="min-w-0">
              <div className="text-[13px] font-medium text-ink">{label}</div>
              <div className="text-[12px] leading-snug text-ink-3">{hint}</div>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
