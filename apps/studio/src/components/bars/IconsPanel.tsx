import { AppWindow, Check, Plus, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { api, errorText, fileUrl, pickImage, pickProgram, type AppIcon, type IconPackInfo } from "../../lib/api";
import { useBars } from "../../lib/barsContext";
import { useStudio } from "../../lib/studio";
import { Button, Group, IconButton, Skeleton } from "../ui";

const MY_PACK = "My icons";

/** Icon packs (docs/bars.md, "Icon packs"): the pack the bars use, and pictures for single apps in one of yours. */
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
  // A pairing can set a pack; "none" then has to say so with null.
  const pairingSetsPack = catalog.presets.find((p) => p.id === resolved.preset)?.bundle.icons != null;
  const choose = (name: string | null) => set(["icons"], name ?? (pairingSetsPack ? null : undefined), "Change the icon pack");
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
    <>
      <Group title="Pack">
        {packs === null ? (
          <Skeleton className="h-24 rounded-lg" />
        ) : (
          <div role="radiogroup" aria-label="Icon pack" data-setting="bars.icons" className="setting-anchor -mx-2 flex flex-col">
            <PackRow title="Built-in" detail="The bars' own icons" on={!inUse} previews={[]} onPick={() => choose(null)} />
            {packs.map((pack) => (
              <PackRow key={pack.name} title={pack.title} detail={packDetail(pack)} on={inUse === pack.name} previews={pack.previews} disabled={!!pack.error} onPick={() => choose(pack.name)} />
            ))}
          </div>
        )}
        {packs && inUse && !current && <p className="text-[12px] leading-snug text-ink">There's no pack called “{inUse}”, so the bars use their own icons.</p>}
        <p className="selectable text-[12px] leading-snug text-ink-3">Packs of your own go in %APPDATA%\mypaper\icons.</p>
      </Group>
      <Group title="Single apps">
        {current && !current.bundled ? (
          <AppIcons key={current.name} pack={current.name} onChanged={refresh} />
        ) : (
          <>
            <p className="text-[12px] leading-snug text-ink-3">Give an app a picture of your own. It goes in a pack of yours; apps it doesn't list keep their icon.</p>
            <Button size="sm" icon={Plus} className="self-start" onClick={() => void startOwnPack()}>
              Start my own pack
            </Button>
          </>
        )}
      </Group>
    </>
  );
}

const packDetail = (pack: IconPackInfo) => pack.error ?? `${pack.bundled ? "Comes with the bars" : "Yours"} · ${pack.entries} ${pack.entries === 1 ? "icon" : "icons"}${pack.apps > 0 ? `, ${pack.apps} for apps` : ""}`;

function PackRow({ title, detail, on, previews, disabled = false, onPick }: { title: string; detail: string; on: boolean; previews: string[]; disabled?: boolean; onPick: () => void }) {
  return (
    <button type="button" role="radio" aria-checked={on} disabled={disabled} onClick={onPick} className="flex cursor-default items-center gap-3 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-surface-hi disabled:opacity-50">
      <span className="min-w-0 flex-1">
        <span className={`block truncate text-[13px] ${on ? "font-medium text-ink" : "text-ink-2"}`}>{title}</span>
        <span className="block truncate text-[12px] text-ink-3">{detail}</span>
      </span>
      {previews.length > 0 && (
        <span className="flex shrink-0 gap-1" aria-hidden>
          {previews.slice(0, 3).map((file) => (
            <img key={file} src={fileUrl(file)} alt="" draggable={false} className="size-4 object-contain opacity-80" />
          ))}
        </span>
      )}
      {on && <Check className="size-4 shrink-0 text-ink" aria-hidden />}
    </button>
  );
}

/** The app pictures in one of the user's packs: add by picking the program, then the picture. */
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
    <>
      {apps === null ? (
        <Skeleton className="h-16 rounded-lg" />
      ) : apps.length === 0 ? (
        <p className="text-[12px] text-ink-3">None yet. Pick a program, then a picture for it.</p>
      ) : (
        <ul className="flex flex-col rounded-lg bg-canvas p-1 ring-1 ring-line-strong ring-inset">
          {apps.map((app) => (
            <li key={app.index} className="group/row flex min-h-9 items-center gap-2 rounded-md px-1.5 hover:bg-surface-hi">
              <span className="relative grid size-6 shrink-0 place-items-center">
                <AppWindow className="absolute size-4 text-ink-3" aria-hidden />
                {/* Over the placeholder: a picture that fails to load just leaves it showing. */}
                <img src={fileUrl(app.file)} alt="" draggable={false} className="relative size-5 object-contain" onError={(e) => (e.currentTarget.style.display = "none")} />
              </span>
              <span className="selectable min-w-0 flex-1 truncate text-[12px] text-ink" title={app.target}>
                {app.kind === "umid" ? app.target : (app.target.split(/[\\/]/).pop() ?? app.target)}
              </span>
              <IconButton icon={X} variant="ghost" size="sm" label={`Remove the picture for ${app.target}`} onClick={() => void remove(app.index)} />
            </li>
          ))}
        </ul>
      )}
      <Button size="sm" icon={Plus} className="self-start" onClick={() => void add()}>
        Add an app
      </Button>
    </>
  );
}
