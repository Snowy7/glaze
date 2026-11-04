import { AppWindow, File, FolderOpen, Globe, Sparkles, TriangleAlert, type LucideIcon } from "lucide-react";
import { useState, type FormEvent } from "react";

import { Stage } from "../components/stage/Stage";
import { ThemePreview } from "../components/ThemeCard";
import { Button, EditorLayout, Inspector, InspectorHeader, InspectorSection, PageBar, Prop, Switch, TextInput, TilePicker, type Tile } from "../components/ui";
import { api, errorText, fileUrl, pickFolder, pickHtml, pickMedia, pickProgram, type Fit, type SourceInfo } from "../lib/api";
import { useMonitors } from "../lib/hooks";
import { KINDS } from "../lib/kinds";
import { useNav } from "../lib/nav";
import { useStudio } from "../lib/studio";

const FITS: readonly { value: Fit; label: string; hint: string }[] = [
  { value: "cover", label: "Fill", hint: "Fill the screen, cropping the edges" },
  { value: "contain", label: "Fit", hint: "Show all of it, with bars at the sides" },
  { value: "stretch", label: "Stretch", hint: "Fill the screen, ignoring its shape" },
  { value: "center", label: "Center", hint: "At its own size, in the middle" },
];

const OBJECT_FIT: Record<Fit, string> = { cover: "object-cover", contain: "object-contain", stretch: "object-fill", center: "object-none" };

const IMAGE = /\.(png|jpe?g|bmp|gif|tiff?|webp|heic|heif)$/i;
const VIDEO = /\.(mp4|mkv|webm|mov|m4v)$/i;

/** New theme: pick a source, see it on the screen, name it, create it. */
export function NewTheme() {
  const { go } = useNav();
  const [source, setSource] = useState<SourceInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function choose(find: () => Promise<SourceInfo | null>) {
    try {
      const info = await find();
      if (!info) return;
      setSource(info);
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <>
      <PageBar title="New theme" back={{ label: "Themes", onClick: () => go({ page: "themes" }) }} />
      {source ? <Details key={source.source} source={source} onChangeSource={() => setSource(null)} /> : <SourcePicker error={error} onChoose={(find) => void choose(find)} />}
    </>
  );
}

const picked = (picker: () => Promise<string | null>) => async () => {
  const path = await picker();
  return path ? api.inspectSource(path) : null;
};

function SourcePicker({ error, onChoose }: { error: string | null; onChoose: (find: () => Promise<SourceInfo | null>) => void }) {
  const [webOpen, setWebOpen] = useState(false);
  const [url, setUrl] = useState("");

  function submitUrl(event: FormEvent) {
    event.preventDefault();
    onChoose(() => api.inspectUrl(url));
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-6 py-12 lg:py-16">
      <div className="mx-auto max-w-3xl">
        <h2 className="font-display text-[26px] leading-tight font-semibold tracking-tight text-ink">What should be on your desktop?</h2>
        <p className="mt-1.5 text-[14px] text-ink-3">Anything below becomes a theme you can use, edit and share.</p>
        <div className="mt-8 grid gap-3 sm:grid-cols-2">
          <SourceTile icon={File} title="A picture or video" detail="Also shaders (.wgsl) and 3D scenes (.glb, .gltf)" onClick={() => onChoose(picked(pickMedia))} />
          <SourceTile icon={FolderOpen} title="A folder of pictures" detail="They cross-fade as a slideshow" onClick={() => onChoose(picked(pickFolder))} />
          <SourceTile icon={Globe} title="A web page" detail="A website or an .html file, live" expanded={webOpen} onClick={() => setWebOpen((open) => !open)} />
          <SourceTile icon={AppWindow} title="A program" detail="One that draws into the desktop, like a Unity game" onClick={() => onChoose(picked(pickProgram))} />
        </div>
        {webOpen && (
          <form onSubmit={submitUrl} className="mt-3 animate-fade-in rounded-xl bg-surface p-4 ring-1 ring-line-strong">
            <div className="flex gap-2">
              <TextInput type="url" inputMode="url" aria-label="Web address" value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://example.com" spellCheck={false} autoFocus className="min-w-0 flex-1" />
              <Button type="submit" variant="primary" disabled={url.trim() === ""}>
                Use this page
              </Button>
            </div>
            <p className="mt-2.5 text-[12.5px] text-ink-3">
              Or{" "}
              <button type="button" onClick={() => onChoose(picked(pickHtml))} className="cursor-default text-ink underline decoration-line-hi underline-offset-2 hover:decoration-ink">
                choose an .html file
              </button>
              .
            </p>
          </form>
        )}
        {error && (
          <p role="alert" className="selectable mt-4 flex items-start gap-2 text-[13px] text-ink">
            <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden />
            {error}
          </p>
        )}
        <ul className="mt-10 flex flex-wrap gap-x-5 gap-y-2 border-t border-line pt-5 text-[12.5px] text-ink-3">
          {Object.values(KINDS).map(({ label, icon: Icon }) => (
            <li key={label} className="flex items-center gap-1.5">
              <Icon className="size-3.5" aria-hidden />
              {label}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function SourceTile({ icon: Icon, title, detail, expanded, onClick }: { icon: LucideIcon; title: string; detail: string; expanded?: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-expanded={expanded}
      className="group flex cursor-default items-center gap-4 rounded-xl bg-surface p-5 text-left ring-1 ring-line-strong transition duration-150 hover:bg-surface-hi hover:ring-line-hi active:scale-[0.99] aria-expanded:bg-surface-hi aria-expanded:ring-line-hi"
    >
      <span className="grid size-12 shrink-0 place-items-center rounded-xl bg-surface-hi text-ink-2 ring-1 ring-line-strong transition-colors group-hover:bg-surface-top group-hover:text-ink">
        <Icon className="size-[22px]" strokeWidth={1.6} aria-hidden />
      </span>
      <span className="min-w-0">
        <span className="block text-[14.5px] font-semibold text-ink">{title}</span>
        <span className="mt-0.5 block text-[12.5px] leading-snug text-ink-3">{detail}</span>
      </span>
    </button>
  );
}

/** The picked source on the screen, and what to call it. */
function Details({ source, onChangeSource }: { source: SourceInfo; onChangeSource: () => void }) {
  const { createTheme, busy } = useStudio();
  const { go } = useNav();
  const monitors = useMonitors();
  const monitor = monitors.find((m) => m.primary) ?? monitors[0];
  const screen = monitor ? { width: monitor.width / monitor.scale, height: monitor.height / monitor.scale } : { width: 1920, height: 1080 };
  const [name, setName] = useState(source.suggestedName);
  const [fit, setFit] = useState<Fit>("cover");
  const [args, setArgs] = useState(source.defaultArgs);
  const [apply, setApply] = useState(true);
  const kind = KINDS[source.kind];

  async function create() {
    const created = await createTheme({ source: source.source, name, fit: source.usesFit ? fit : null, args: source.kind === "app" ? args : null, apply });
    if (created) go(apply ? { page: "desktop" } : { page: "themes" });
  }

  const fitTiles = FITS.map((option): Tile<Fit> => ({ value: option.value, label: option.label, hint: option.hint, preview: <FitPicture fit={option.value} /> }));

  return (
    <EditorLayout
      stage={
        <Stage label="Preview" screen={screen}>
          <SourcePreview source={source} fit={fit} name={name} />
        </Stage>
      }
      inspector={
        <Inspector label="New theme">
          <InspectorHeader icon={kind.icon} title={`${kind.label} theme`} detail={<span className="selectable font-mono text-[11.5px]">{source.source}</span>} />
          <InspectorSection>
            <Prop label="Name" stacked>
              <TextInput compact aria-label="Name" value={name} maxLength={60} onChange={(e) => setName(e.currentTarget.value)} className="w-full" />
            </Prop>
            {source.usesFit && (
              <Prop label="On the screen" stacked hint={FITS.find((f) => f.value === fit)?.hint}>
                <TilePicker<Fit> label="How it fills the screen" value={fit} options={fitTiles} onChange={setFit} min="4.5rem" />
              </Prop>
            )}
            {source.kind === "app" && (
              <Prop label="Start it with" stacked hint="{hwnd} becomes the desktop window the program draws into.">
                <TextInput compact mono aria-label="Arguments" value={args} onChange={(e) => setArgs(e.currentTarget.value)} placeholder="-parentHWND {hwnd} delayed" className="w-full" />
                {source.presets.length > 0 && (
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {source.presets.map((preset) => (
                      <Button key={preset.id} size="sm" variant={args === preset.args ? "primary" : "secondary"} onClick={() => setArgs(preset.args)}>
                        {preset.label}
                      </Button>
                    ))}
                  </div>
                )}
              </Prop>
            )}
            <Prop label="Use it right away">
              <Switch label="Use it right away" checked={apply} onChange={setApply} />
            </Prop>
          </InspectorSection>
          <InspectorSection>
            <div className="flex gap-2">
              <Button variant="ghost" onClick={onChangeSource}>
                Change source
              </Button>
              <Button variant="primary" icon={Sparkles} className="flex-1" busy={busy.has("create")} disabled={name.trim() === ""} onClick={() => void create()}>
                {apply ? "Create and use" : "Create theme"}
              </Button>
            </div>
          </InspectorSection>
        </Inspector>
      }
    />
  );
}

/** The real picture or video when there is one; otherwise the theme's placeholder art. */
function SourcePreview({ source, fit, name }: { source: SourceInfo; fit: Fit; name: string }) {
  const [failed, setFailed] = useState(false);
  const className = `absolute inset-0 size-full ${OBJECT_FIT[fit]}`;
  if (!failed && IMAGE.test(source.source)) return <img src={fileUrl(source.source)} alt="" className={className} onError={() => setFailed(true)} />;
  if (!failed && VIDEO.test(source.source)) return <video src={fileUrl(source.source)} className={className} autoPlay muted loop playsInline onError={() => setFailed(true)} />;
  return (
    <div className="absolute inset-0">
      <ThemePreview name={name || "Untitled"} kind={source.kind} preview={null} />
      {source.kind === "web" && <span className="absolute inset-x-0 bottom-[12%] text-center font-mono text-[22px] text-white/60">{source.source}</span>}
    </div>
  );
}

/** A tiny landscape, laid out the way each fit lays out a picture. */
function FitPicture({ fit }: { fit: Fit }) {
  const aspect = { cover: "xMidYMid slice", contain: "xMidYMid meet", stretch: "none", center: "xMidYMid meet" }[fit];
  return (
    <span className="absolute inset-0 grid place-items-center bg-[#1d1d1d]">
      <svg viewBox="0 0 40 30" preserveAspectRatio={aspect} className={fit === "center" ? "h-1/2 w-1/2" : "size-full"} aria-hidden>
        <rect width="40" height="30" fill="#8a8a8a" />
        <circle cx="29" cy="9" r="4" fill="#e6e6e6" />
        <path d="M0 30 L13 12 L21 22 L27 16 L40 30 Z" fill="#3d3d3d" />
      </svg>
    </span>
  );
}
