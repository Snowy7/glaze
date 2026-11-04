import { FolderOpen, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

import { pickAnyFile, pickImage } from "../../lib/api";
import { tidy } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import { customId, isCustom, TRIGGERS, type Action, type ModuleOptions, type ModuleRef, type Trigger } from "../../lib/barsSchema";
import { describeModule } from "../../lib/modules";
import { Block, ConfirmButton, Group, IconButton, INPUT_CLASS, Row, Slider } from "../ui";
import { ActionPicker } from "./ActionPicker";
import { PaintValue, ColorValue } from "./PaintEditor";

const TRIGGER_LABELS: Record<Trigger, string> = {
  left: "Left click",
  middle: "Middle click",
  right: "Right click",
  wheelUp: "Wheel up",
  wheelDown: "Wheel down",
};

const complete = (action: Action | undefined): boolean =>
  action === undefined || typeof action === "string" || ("exec" in action ? action.exec.trim() !== "" : action.open.trim() !== "");

/** One input's action. A half-filled program or path stays in the box and is written once it names something. */
function ClickRow({ label, value, onCommit }: { label: string; value: Action | undefined; onCommit: (value: Action | undefined) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <Block title={label}>
      <ActionPicker
        label={label}
        value={draft}
        onChange={(next) => {
          setDraft(next);
          if (complete(next)) onCommit(next);
        }}
      />
    </Block>
  );
}

/** Options of the selected module: clicks, look, icon, and for a custom module its command. */
export function ModuleOptionsEditor({ moduleRef, onRemoved }: { moduleRef: ModuleRef; onRemoved: () => void }) {
  const { resolved, set, update } = useBars();
  const { label, icon: Icon } = describeModule(moduleRef);
  const options: ModuleOptions = resolved.modules[moduleRef] ?? {};
  const custom = isCustom(moduleRef);
  const at = (...rest: string[]) => ["modules", moduleRef, ...rest] as const;

  function remove() {
    update((doc) => {
      const { [moduleRef]: _gone, ...modules } = doc.modules ?? {};
      const strip = (list: ModuleRef[] | undefined) => list?.filter((ref) => ref !== moduleRef);
      const { modules: _old, ...rest } = doc;
      return {
        ...rest,
        ...(Object.keys(modules).length > 0 ? { modules } : {}),
        ...(doc.top && { top: { ...doc.top, ...(doc.top.left && { left: strip(doc.top.left) }), ...(doc.top.center && { center: strip(doc.top.center) }), ...(doc.top.right && { right: strip(doc.top.right) }) } }),
        ...(doc.dock && { dock: { ...doc.dock, ...(doc.dock.left && { left: strip(doc.dock.left) }), ...(doc.dock.right && { right: strip(doc.dock.right) }) } }),
      };
    });
    onRemoved();
  }

  return (
    <Group
      title={custom ? `Custom module: ${customId(moduleRef)}` : `${label} options`}
      action={
        custom ? (
          <ConfirmButton icon={Trash2} label={`Remove ${moduleRef}`} onConfirm={remove}>
            Remove
          </ConfirmButton>
        ) : undefined
      }
    >
      <div className="flex items-center gap-3 px-5 py-3 text-[13px] text-ink-3">
        <Icon className="size-4 text-ink-2" aria-hidden />
        {describeModule(moduleRef).hint}
      </div>

      {custom && <CustomFields moduleRef={moduleRef} options={options} />}

      {TRIGGERS.map((trigger) => (
        <ClickRow key={`${moduleRef}:${trigger}`} label={TRIGGER_LABELS[trigger]} value={options.click?.[trigger]} onCommit={(action) => set(at("click", trigger), action)} />
      ))}

      <Block title="Icon" detail="An icon-pack key, or an image file (svg, png, ico). It replaces the module's glyph; SVGs are drawn in the text color.">
        <div className="flex gap-2">
          <input aria-label="Icon" value={options.icon ?? ""} placeholder="wifi-3, or a file path" spellCheck={false} onChange={(e) => set(at("icon"), e.currentTarget.value === "" ? undefined : e.currentTarget.value)} className={`${INPUT_CLASS} max-w-96 flex-1 font-mono text-[13px]`} />
          <IconButton icon={FolderOpen} label="Choose an icon file" onClick={() => void pickImage().then((path) => path && set(at("icon"), path))} />
        </div>
      </Block>

      <Block title="Look" detail="Overrides for this module only. Unset values follow the bar's look.">
        <StyleFields moduleRef={moduleRef} options={options} />
      </Block>
    </Group>
  );
}

function CustomFields({ moduleRef, options }: { moduleRef: ModuleRef; options: ModuleOptions }) {
  const { set } = useBars();
  const at = (key: string) => ["modules", moduleRef, key] as const;
  const [command, setCommand] = useState(options.command ?? "");
  useEffect(() => setCommand(options.command ?? ""), [options.command, moduleRef]);
  const [argsText, setArgsText] = useState((options.args ?? []).join("\n"));
  // Another module was selected: show its arguments.
  useEffect(() => setArgsText((options.args ?? []).join("\n")), [moduleRef]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <>
      <Block title="Command" detail="The program whose output the module shows: its first line as text, or a JSON object { text, icon, tooltip, class }. It runs with no console window and a 10 second limit.">
        <div className="flex gap-2">
          <input
            aria-label="Command"
            value={command}
            placeholder="curl.exe"
            spellCheck={false}
            onChange={(e) => {
              setCommand(e.currentTarget.value);
              // The host refuses a custom module without a command, so an empty box isn't written.
              if (e.currentTarget.value.trim() !== "") set(at("command"), e.currentTarget.value);
            }}
            className={`${INPUT_CLASS} flex-1 font-mono text-[13px]`}
          />
          <IconButton icon={FolderOpen} label="Choose the program" onClick={() => void pickAnyFile().then((path) => path && set(at("command"), path))} />
        </div>
        {command.trim() === "" && <p className="mt-2 text-xs text-ink-3">A custom module needs a command; this isn't saved while it's empty.</p>}
        <label className="mt-3 block text-[12.5px] font-medium text-ink-2" htmlFor={`args-${moduleRef}`}>
          Arguments, one per line
        </label>
        <textarea
          id={`args-${moduleRef}`}
          rows={3}
          value={argsText}
          spellCheck={false}
          onChange={(e) => {
            // The box keeps what is typed (a new empty line is just the cursor); the file gets the non-empty lines.
            setArgsText(e.currentTarget.value);
            const args = e.currentTarget.value.split("\n").filter((line) => line !== "");
            set(at("args"), args.length === 0 ? undefined : args);
          }}
          className={`${INPUT_CLASS} mt-1.5 h-auto py-2 font-mono text-[13px]`}
        />
      </Block>
      <Row title="Refresh every" detail="Seconds between runs, 1 to 86400. It only runs while a bar on screen shows it, and pauses while a fullscreen app is in front.">
        <Slider className="w-64" label="Refresh interval" value={options.interval ?? 60} min={1} max={3600} step={1} format={(v) => (v >= 60 && v % 60 === 0 ? `${v / 60} min` : `${v} s`)} onChange={(v) => set(at("interval"), v === 60 ? undefined : v)} />
      </Row>
    </>
  );
}

function StyleFields({ moduleRef, options }: { moduleRef: ModuleRef; options: ModuleOptions }) {
  const { set } = useBars();
  const style = options.style ?? {};
  const at = (key: string) => ["modules", moduleRef, "style", key] as const;
  const optional = (key: "padding" | "radius" | "size" | "weight", label: string, min: number, max: number, step: number, fallback: number) => (
    <div className="flex items-center gap-3">
      <span className="w-28 shrink-0 text-[13px] text-ink-2">{label}</span>
      <Slider className={`min-w-0 flex-1 ${style[key] === undefined ? "opacity-55" : ""}`} label={label} value={style[key] ?? fallback} min={min} max={max} step={step} onChange={(v) => set(at(key), tidy(v))} />
      <button type="button" disabled={style[key] === undefined} onClick={() => set(at(key), undefined)} className="w-12 shrink-0 cursor-default text-xs text-ink-3 enabled:hover:text-ink">
        {style[key] === undefined ? "Default" : "Reset"}
      </button>
    </div>
  );
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <span className="w-28 shrink-0 text-[13px] text-ink-2">Text color</span>
        <ColorValue label="Text color" value={style.color} onChange={(color) => set(at("color"), color)} onClear={() => set(at("color"), undefined)} />
      </div>
      <div className="flex items-start gap-3">
        <span className="mt-2 w-28 shrink-0 text-[13px] text-ink-2">Background</span>
        <div className="min-w-0 flex-1">
          <PaintValue label="Module background" value={style.background} onChange={(paint) => set(at("background"), paint)} onClear={() => set(at("background"), undefined)} placeholder="#ffffff1a" />
        </div>
      </div>
      {optional("padding", "Padding", 0, 32, 1, 8)}
      {optional("radius", "Corner radius", 0, 32, 1, 6)}
      {optional("size", "Font size", 8, 32, 1, 13)}
      {optional("weight", "Font weight", 100, 900, 50, 400)}
    </div>
  );
}
