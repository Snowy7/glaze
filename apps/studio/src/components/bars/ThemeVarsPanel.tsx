import { useEffect, useState } from "react";

import { api, errorText, onChanged, type ThemeVar, type ThemeVars } from "../../lib/api";
import { useBars } from "../../lib/barsContext";
import type { Json } from "../../lib/barsSchema";
import { Group, Row, Select, Slider, Switch } from "../ui";
import { ColorValue } from "./PaintEditor";

/** The active theme's bars variables as typed controls (docs/bars.md, "Theme variables"). */
export function ThemeVarsPanel() {
  const [found, setFound] = useState<ThemeVars | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    const refresh = () =>
      void api.themeVars().then(
        (vars) => current && setFound(vars),
        (e: unknown) => current && setFailure(errorText(e)),
      );
    refresh();
    // The theme (or its style sheet) can change while this page is open.
    const unlisten = onChanged((changes) => (changes.config || changes.themes) && refresh());
    return () => {
      current = false;
      void unlisten.then((stop) => stop());
    };
  }, []);

  if (failure) return <Group title="Theme variables"><Row title="Couldn't read the theme" detail={failure} /></Group>;
  if (!found) return null;

  // Variables keep their declared group; ungrouped ones come first.
  const groups = [...new Set(found.vars.map((v) => v.group ?? ""))].sort((a, b) => (a === "" ? -1 : b === "" ? 1 : 0));

  return (
    <div className="flex flex-col gap-6">
      {found.error && (
        <Group title="Theme variables">
          <Row title="The theme's variables can't be read" detail={found.error} />
        </Group>
      )}
      {groups.map((group) => (
        <Group key={group} title={group === "" ? `Theme variables · ${found.theme}` : group}>
          {found.vars.filter((v) => (v.group ?? "") === group).map((v) => (
            <VarRow key={v.name} decl={v} />
          ))}
        </Group>
      ))}
    </div>
  );
}

const fromDefault = (value: Json | undefined, fallback: Json): Json => value ?? fallback;

function VarRow({ decl }: { decl: ThemeVar }) {
  const { doc, set } = useBars();
  const own = doc.vars?.[decl.name];
  const value = fromDefault(own, decl.default);
  const write = (next: Json) => set(["vars", decl.name], next);
  const unit = decl.type === "length" ? decl.unit : "";

  return (
    <Row
      title={decl.label}
      detail={
        <>
          {decl.description ?? <code className="font-mono text-[11.5px]">{decl.name}</code>}
          {own !== undefined && (
            <button type="button" onClick={() => set(["vars", decl.name], undefined)} className="ml-2 cursor-default rounded px-1 text-[11.5px] font-medium text-ink-2 underline decoration-line-hi underline-offset-2 hover:text-ink">
              Reset
            </button>
          )}
        </>
      }
    >
      {decl.type === "color" && typeof value === "string" && <ColorValue hexOnly label={decl.label} value={value} onChange={write} />}
      {(decl.type === "number" || decl.type === "length") && typeof value === "number" && (
        <Slider className="w-64" label={decl.label} value={value} min={decl.min ?? 0} max={decl.max ?? 100} step={decl.step ?? 1} format={(v) => `${v}${unit}`} onChange={write} />
      )}
      {decl.type === "select" && typeof value === "string" && (
        <Select<string> label={decl.label} className="w-48" value={value} options={(decl.options ?? []).map((o) => ({ value: o, label: o }))} onChange={write} />
      )}
      {decl.type === "bool" && <Switch label={decl.label} checked={value === true} onChange={write} />}
    </Row>
  );
}
