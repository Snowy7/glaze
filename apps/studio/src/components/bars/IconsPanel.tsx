import { AppWindow, Check, Plus, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText, fileUrl, pickImage, pickProgram, type AppIcon, type IconPackInfo } from "../../lib/api";
import { useBars } from "../../lib/barsContext";
import { useStudio } from "../../lib/studio";
import { Block, Button, Group, IconButton, Note, Skeleton } from "../ui";

const MY_PACK = "My icons";

/** Icon packs (docs/bars.md, "Icon packs"): choose the pack the bars use, and override single apps' icons. */
export function IconsPanel() {
  const { resolved, catalog, set } = useBars();
  const { notify } = useStudio();
  const [packs, setPacks] = useState<IconPackInfo[] | null>(null);

  const refresh = useCallback(async () => {
    try {
      setPacks(await api.listIconPacks());
    } catch (e) {
      notify(errorText(e), "error");
    }
  }, [notify]);

  useEffect(() => {
    void refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  const inUse = resolved.icons;
  /** A preset can set a pack (windows11 uses mono); "none" then has to say so with null. */
  const presetSetsPack = catalog.presets.find((p) => p.id === resolved.preset)?.bundle.icons != null;
  const choose = (name: string | null) => set(["icons"], name ?? (presetSetsPack ? null : undefined));
  const current = packs?.find((p) => p.name === inUse);

  async function startOwnPack() {
    try {
      await api.createIconPack(MY_PACK);
      await refresh();
      choose(MY_PACK);
    } catch (e) {
      notify(errorText(e), "error");
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <Group title="Icon pack">
        <Block title="Pack in use" detail="Replaces the bars' built-in glyphs (Start, Wi-Fi, volume…) and, for apps the pack lists, the apps' own icons.">
          {packs === null ? (
            <Skeleton className="h-28 rounded-xl" />
          ) : (
            <ul className="grid grid-cols-2 gap-3" role="radiogroup" aria-label="Icon pack">
              <PackCard title="Built-in glyphs" detail="No pack: the bars' own icons and each app's own icon." on={!inUse} onPick={() => choose(null)} previews={[]} />
              {packs.map((pack) => (
                <PackCard key={pack.name} title={pack.title} detail={packDetail(pack)} on={inUse === pack.name} previews={pack.previews} error={pack.error} onPick={() => choose(pack.name)} />
              ))}
            </ul>
          )}
          {packs && inUse && !current && <p className="mt-3 rounded-lg bg-canvas px-3 py-2 text-[13px] text-ink ring-1 ring-line-hi">There is no pack called “{inUse}”, so the bars use their built-in glyphs. Choose another, or put it in %APPDATA%\mypaper\icons\{inUse}\.</p>}
          <Note>
            Your own packs live in <code className="font-mono text-xs">%APPDATA%\mypaper\icons\&lt;pack&gt;\</code> as an <code className="font-mono text-xs">icons.json</code> and the images it names. The Windows 11 preset uses the bundled mono pack.
          </Note>
        </Block>
      </Group>

      <Group title="Icons for single apps">
        {current && !current.bundled ? (
          <AppIcons key={current.name} pack={current.name} onChanged={refresh} />
        ) : (
          <Block title="Give an app its own icon" detail="App icons are entries of a pack, so this edits one of yours.">
            <p className="text-[13px] text-ink-3">
              {current?.bundled ? `${current.title} ships with the bars and can't be edited.` : "Built-in glyphs have no app entries."} Start a pack of your own and use it; apps it doesn't list keep their own icon, and the glyphs stay the built-in ones.
            </p>
            <Button className="mt-3" icon={Plus} onClick={() => void startOwnPack()}>
              Create “{MY_PACK}” and use it
            </Button>
          </Block>
        )}
      </Group>
    </div>
  );
}

const packDetail = (pack: IconPackInfo) =>
  pack.error ?? `${pack.bundled ? "Bundled" : "Yours"} · ${pack.entries} ${pack.entries === 1 ? "icon" : "icons"}${pack.apps > 0 ? `, ${pack.apps} for apps` : ""}${pack.mono ? " · drawn in the text color" : ""}`;

function PackCard({ title, detail, on, previews, error, onPick }: { title: string; detail: string; on: boolean; previews: string[]; error?: string | null; onPick: () => void }) {
  return (
    <li>
      <button
        type="button"
        role="radio"
        aria-checked={on}
        disabled={!!error}
        onClick={onPick}
        className={`flex h-full w-full cursor-default flex-col rounded-xl p-3 text-left ring-1 transition disabled:opacity-50 ${on ? "bg-solid text-solid-ink ring-transparent" : "bg-canvas text-ink-2 ring-line-strong hover:ring-line-hi"}`}
      >
        <span className="flex items-center justify-between gap-2">
          <span className="truncate text-[13.5px] font-semibold">{title}</span>
          {on && <Check className="size-4 shrink-0" aria-hidden />}
        </span>
        <span className={`mt-0.5 text-[12px] ${on ? "opacity-75" : "text-ink-3"}`}>{detail}</span>
        {previews.length > 0 && (
          <span className="mt-2 flex gap-1.5">
            {previews.map((file) => (
              <span key={file} className={`grid size-8 place-items-center rounded-md ${on ? "bg-solid-ink/15" : "bg-surface-hi"}`}>
                <img src={fileUrl(file)} alt="" draggable={false} className={`size-5 object-contain ${on ? "invert" : ""}`} />
              </span>
            ))}
          </span>
        )}
      </button>
    </li>
  );
}

/** The app entries of one of the user's packs: add by picking the program and a picture. */
function AppIcons({ pack, onChanged }: { pack: string; onChanged: () => Promise<void> }) {
  const { notify } = useStudio();
  const [apps, setApps] = useState<AppIcon[] | null>(null);

  const load = useCallback(async () => {
    try {
      setApps(await api.iconPackApps(pack));
    } catch (e) {
      notify(errorText(e), "error");
    }
  }, [pack, notify]);

  useEffect(() => void load(), [load]);

  async function add() {
    const program = await pickProgram();
    if (!program) return;
    const image = await pickImage();
    if (!image) return;
    try {
      await api.setAppIcon(pack, program, image);
      await Promise.all([load(), onChanged()]);
    } catch (e) {
      notify(errorText(e), "error");
    }
  }

  async function remove(index: number) {
    try {
      await api.removeAppIcon(pack, index);
      await Promise.all([load(), onChanged()]);
    } catch (e) {
      notify(errorText(e), "error");
    }
  }

  return (
    <Block title={`App icons in “${pack}”`} detail="Matched by the program's full path. Store apps and file types can be added in the pack's icons.json.">
      {apps === null ? (
        <Skeleton className="h-16 rounded-xl" />
      ) : apps.length === 0 ? (
        <p className="rounded-xl border border-dashed border-line-strong px-4 py-5 text-center text-[13px] text-ink-3">No app icons yet. Pick a program, then a picture for it.</p>
      ) : (
        <ul className="divide-y divide-line overflow-hidden rounded-xl bg-canvas ring-1 ring-line">
          {apps.map((app) => (
            <li key={app.index} className="flex items-center gap-3 px-3 py-2">
              <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-hi">
                <img src={fileUrl(app.file)} alt="" draggable={false} className="size-6 object-contain" onError={(e) => (e.currentTarget.style.display = "none")} />
                <AppWindow className="absolute size-4 text-ink-3 opacity-0" aria-hidden />
              </span>
              <div className="min-w-0 flex-1">
                <div className="selectable truncate font-mono text-[12.5px] text-ink" title={app.target}>
                  {app.target}
                </div>
                <div className="text-[11.5px] text-ink-3">{app.kind === "umid" ? "Store app" : "Program"}</div>
              </div>
              <IconButton icon={X} label={`Remove the icon for ${app.target}`} onClick={() => void remove(app.index)} />
            </li>
          ))}
        </ul>
      )}
      <Button icon={Plus} className="mt-3" onClick={() => void add()}>
        Add an app icon
      </Button>
    </Block>
  );
}
