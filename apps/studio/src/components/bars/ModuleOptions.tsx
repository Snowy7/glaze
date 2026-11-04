import { FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";

import { pickAnyFile, pickImage } from "../../lib/api";
import { tidy } from "../../lib/barsDoc";
import { useBars } from "../../lib/barsContext";
import { isCustom, TRIGGERS, type Action, type BarsDoc, type ModuleOptions, type ModuleRef, type Trigger } from "../../lib/barsSchema";
import { IconButton, INPUT_BASE, InspectorSection, More, Prop, Slider } from "../ui";
import { ActionPicker } from "./ActionPicker";
import { ColorValue, PaintValue } from "./PaintEditor";

// A module's own options (docs/bars.md, "Per-module options"), as inspector sections: what a click
// does first, the other inputs and its icon and look one step away, and for a custom module the
// command it runs.

const TRIGGER_LABELS: Record<Trigger, string> = {
  left: "Click",
  middle: "Middle click",
  right: "Right click",
  wheelUp: "Scroll up",
  wheelDown: "Scroll down",
};

const complete = (action: Action | undefined): boolean =>
  action === undefined || typeof action === "string" || ("exec" in action ? action.exec.trim() !== "" : action.open.trim() !== "");

/** `doc` without `ref` anywhere: its options and every list it was in (deleting a custom module). */
export function withoutModule(doc: BarsDoc, ref: ModuleRef): BarsDoc {
  const { [ref]: _gone, ...modules } = doc.modules ?? {};
  const { modules: _old, ...rest } = doc;
  const strip = (list: ModuleRef[] | undefined) => list?.filter((entry) => entry !== ref);
  return {
    ...rest,
    ...(Object.keys(modules).length > 0 ? { modules } : {}),
    ...(doc.top && { top: { ...doc.top, ...(doc.top.left && { left: strip(doc.top.left) }), ...(doc.top.center && { center: strip(doc.top.center) }), ...(doc.top.right && { right: strip(doc.top.right) }) } }),
    ...(doc.dock && { dock: { ...doc.dock, ...(doc.dock.left && { left: strip(doc.dock.left) }), ...(doc.dock.right && { right: strip(doc.dock.right) }) } }),
  };
}

/** One input's action. A half-filled program or path stays in the box and is written once it names something. */
function ClickProp({ moduleRef, trigger, value }: { moduleRef: ModuleRef; trigger: Trigger; value: Action | undefined }) {
  const { set, has } = useBars();
  const path = ["modules", moduleRef, "click", trigger];
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const label = TRIGGER_LABELS[trigger];
  return (
    <Prop id={`bars.module.click.${trigger}`} label={label} stacked inherited={has(path) ? undefined : "Default"} onReset={has(path) ? () => set(path, undefined) : undefined}>
      <ActionPicker
        label={label}
        value={draft}
        onChange={(next) => {
          setDraft(next);
          if (complete(next)) set(path, next, `Change what a ${label.toLowerCase()} does`);
        }}
      />
    </Prop>
  );
}

/** The module's options: clicks, then (for a custom module) its command, then icon and look. */
export function ModuleFields({ moduleRef }: { moduleRef: ModuleRef }) {
  const { resolved, set } = useBars();
  const options: ModuleOptions = resolved.modules[moduleRef] ?? {};
  const at = (...rest: string[]) => ["modules", moduleRef, ...rest];
  const others = TRIGGERS.filter((t) => t !== "left");

  return (
    <>
      {isCustom(moduleRef) && <CustomFields moduleRef={moduleRef} options={options} />}
      <InspectorSection title="When clicked">
        <ClickProp moduleRef={moduleRef} trigger="left" value={options.click?.left} />
      </InspectorSection>
      <More label="Other clicks and scrolling">
        {others.map((trigger) => (
          <ClickProp key={`${moduleRef}:${trigger}`} moduleRef={moduleRef} trigger={trigger} value={options.click?.[trigger]} />
        ))}
      </More>
      <More label="Icon and look">
        <Prop id="bars.module.icon" label="Icon" hint="An icon-pack name, or a picture." stacked inherited={options.icon === undefined ? "Default" : undefined} onReset={options.icon === undefined ? undefined : () => set(at("icon"), undefined)}>
          <div className="flex gap-2">
            <input aria-label="Icon" value={options.icon ?? ""} placeholder="wifi-3, or a file" spellCheck={false} onChange={(e) => set(at("icon"), e.currentTarget.value === "" ? undefined : e.currentTarget.value)} className={`${INPUT_BASE} ui-input-sm min-w-0 flex-1 font-mono text-[12px]`} />
            <IconButton icon={FolderOpen} size="sm" label="Choose a picture" onClick={() => void pickImage().then((path) => path && set(at("icon"), path))} />
          </div>
        </Prop>
        <StyleFields moduleRef={moduleRef} options={options} />
      </More>
    </>
  );
}

function CustomFields({ moduleRef, options }: { moduleRef: ModuleRef; options: ModuleOptions }) {
  const { set } = useBars();
  const at = (key: string) => ["modules", moduleRef, key];
  const [command, setCommand] = useState(options.command ?? "");
  useEffect(() => setCommand(options.command ?? ""), [options.command, moduleRef]);
  const [argsText, setArgsText] = useState((options.args ?? []).join("\n"));
  // Another module was selected: show its arguments.
  useEffect(() => setArgsText((options.args ?? []).join("\n")), [moduleRef]);

  return (
    <InspectorSection title="What it shows">
      <Prop id="bars.module.command" label="Program" hint={command.trim() === "" ? "Needed: nothing is saved without one." : "Its first line of output becomes the module's text."} stacked>
        <div className="flex gap-2">
          <input
            aria-label="Program"
            value={command}
            placeholder="curl.exe"
            spellCheck={false}
            onChange={(e) => {
              setCommand(e.currentTarget.value);
              // The host refuses a custom module without a command, so an empty box isn't written.
              if (e.currentTarget.value.trim() !== "") set(at("command"), e.currentTarget.value);
            }}
            className={`${INPUT_BASE} ui-input-sm min-w-0 flex-1 font-mono text-[12px]`}
          />
          <IconButton icon={FolderOpen} size="sm" label="Choose the program" onClick={() => void pickAnyFile().then((path) => path && set(at("command"), path))} />
        </div>
      </Prop>
      <Prop id="bars.module.args" label="Arguments" hint="One per line." stacked>
        <textarea
          aria-label="Arguments"
          rows={3}
          value={argsText}
          spellCheck={false}
          onChange={(e) => {
            // The box keeps what is typed (a new empty line is just the cursor); the file gets the non-empty lines.
            setArgsText(e.currentTarget.value);
            const args = e.currentTarget.value.split("\n").filter((line) => line !== "");
            set(at("args"), args.length === 0 ? undefined : args);
          }}
          className={`${INPUT_BASE} h-auto w-full py-2 font-mono text-[12px]`}
        />
      </Prop>
      <Prop id="bars.module.interval" label="Run every">
        <Slider className="w-[10.5rem] 2xl:w-[13rem]" label="Run every" value={options.interval ?? 60} min={1} max={3600} step={1} format={(v) => (v >= 60 && v % 60 === 0 ? `${v / 60} min` : `${v} s`)} onChange={(v) => set(at("interval"), v === 60 ? undefined : v)} />
      </Prop>
    </InspectorSection>
  );
}

function StyleFields({ moduleRef, options }: { moduleRef: ModuleRef; options: ModuleOptions }) {
  const { set } = useBars();
  const style = options.style ?? {};
  const at = (key: string) => ["modules", moduleRef, "style", key];
  const reset = (key: keyof typeof style) => (style[key] === undefined ? undefined : () => set(at(key), undefined));
  const slider = (key: "padding" | "radius" | "size" | "weight", label: string, min: number, max: number, step: number, fallback: number) => (
    <Prop id={`bars.module.style.${key}`} label={label} inherited={style[key] === undefined ? "Bar" : undefined} onReset={reset(key)}>
      <Slider className="w-[10.5rem] 2xl:w-[13rem]" label={label} value={style[key] ?? fallback} min={min} max={max} step={step} onChange={(v) => set(at(key), tidy(v))} />
    </Prop>
  );
  return (
    <>
      <Prop id="bars.module.style.color" label="Text color" stacked inherited={style.color === undefined ? "Bar" : undefined} onReset={reset("color")}>
        <ColorValue label="Text color" value={style.color} onChange={(color) => set(at("color"), color)} />
      </Prop>
      <Prop id="bars.module.style.background" label="Background" stacked inherited={style.background === undefined ? "Bar" : undefined} onReset={reset("background")}>
        <PaintValue label="Module background" value={style.background} onChange={(paint) => set(at("background"), paint)} placeholder="#ffffff1a" />
      </Prop>
      {slider("padding", "Padding", 0, 32, 1, 8)}
      {slider("radius", "Corner radius", 0, 32, 1, 6)}
      {slider("size", "Text size", 8, 32, 1, 13)}
      {slider("weight", "Text weight", 100, 900, 50, 400)}
    </>
  );
}
