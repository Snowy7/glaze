import { FolderOpen } from "lucide-react";

import { pickAnyFile } from "../../lib/api";
import { BUILTIN_ACTIONS, type Action, type BuiltinAction } from "../../lib/barsSchema";
import { IconButton, INPUT_BASE, Segmented, Select } from "../ui";

const ACTION_LABELS: Record<BuiltinAction, string> = {
  default: "The module's own behavior",
  none: "Nothing",
  start: "Open the native Start menu",
  search: "Open Windows Search",
  notifications: "Open the notifications flyout",
  "quick-settings": "Open quick settings",
  "task-manager": "Open Task Manager",
  settings: "Open Windows Settings",
  "show-desktop": "Show the desktop",
  "media-toggle": "Play or pause",
  "media-next": "Next track",
  "media-previous": "Previous track",
  "volume-up": "Volume up",
  "volume-down": "Volume down",
  "volume-mute": "Mute",
  "workspace-next": "Next virtual desktop",
  "workspace-previous": "Previous virtual desktop",
  lock: "Lock the PC",
  explorer: "Open File Explorer",
};

type Kind = "own" | "builtin" | "exec" | "open";

const kindOf = (action: Action | undefined): Kind =>
  action === undefined ? "own" : typeof action === "string" ? "builtin" : "exec" in action ? "exec" : "open";

type Props = {
  label: string;
  value: Action | undefined;
  /** `undefined` leaves the input to the module's own behavior. */
  onChange: (value: Action | undefined) => void;
};

/**
 * What one click or wheel notch does: the module's own behavior, a built-in action, a program with
 * arguments, or a path or link to open. The host runs `exec` and `open` from its own copy of the
 * config; the bar's page only ever asks for "this module, this input".
 */
export function ActionPicker({ label, value, onChange }: Props) {
  const kind = kindOf(value);

  function pick(next: Kind) {
    if (next === kind) return;
    onChange(next === "own" ? undefined : next === "builtin" ? "none" : next === "exec" ? { exec: "", args: [] } : { open: "" });
  }

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <Segmented<Kind>
          className="w-[22rem]"
          label={`${label} action`}
          hideLabel
          value={kind}
          onChange={pick}
          options={[
            { value: "own", label: "Default", hint: "Keep the module's own behavior" },
            { value: "builtin", label: "Action" },
            { value: "exec", label: "Program" },
            { value: "open", label: "Open" },
          ]}
        />
        {typeof value === "string" && (
          <Select<BuiltinAction>
            label={`${label} built-in action`}
            className="w-64"
            value={value}
            options={BUILTIN_ACTIONS.map((action) => ({ value: action, label: ACTION_LABELS[action] }))}
            onChange={onChange}
          />
        )}
      </div>
      {value !== undefined && typeof value === "object" && "exec" in value && (
        <div className="flex flex-wrap gap-2">
          <div className="flex min-w-0 flex-1 gap-2">
            <input aria-label={`${label} program`} value={value.exec} placeholder="notepad.exe" spellCheck={false} onChange={(e) => onChange({ ...value, exec: e.currentTarget.value })} className={`${INPUT_BASE} min-w-48 flex-1 font-mono text-[13px]`} />
            <IconButton icon={FolderOpen} label={`Choose the program for ${label}`} onClick={() => void pickAnyFile().then((path) => path && onChange({ ...value, exec: path }))} />
          </div>
          <input
            aria-label={`${label} arguments`}
            value={(value.args ?? []).join(" ")}
            placeholder="Arguments, separated by spaces"
            spellCheck={false}
            // One argument per space-separated word: quote nothing, the host passes them as a list.
            onChange={(e) => onChange({ ...value, args: e.currentTarget.value.split(" ").filter((word) => word !== "") })}
            className={`${INPUT_BASE} min-w-48 flex-1 font-mono text-[13px]`}
          />
        </div>
      )}
      {value !== undefined && typeof value === "object" && "open" in value && (
        <div className="flex gap-2">
          <input aria-label={`${label} path or link`} value={value.open} placeholder="https://example.com, ms-settings:display or a path" spellCheck={false} onChange={(e) => onChange({ open: e.currentTarget.value })} className={`${INPUT_BASE} min-w-48 flex-1 font-mono text-[13px]`} />
          <IconButton icon={FolderOpen} label={`Choose the file for ${label}`} onClick={() => void pickAnyFile().then((path) => path && onChange({ open: path }))} />
        </div>
      )}
      {value !== undefined && typeof value === "object" && ("exec" in value ? value.exec.trim() === "" : value.open.trim() === "") && (
        <p className="text-xs text-ink-3">Not saved until it names a {"exec" in value ? "program" : "path or link"}.</p>
      )}
    </div>
  );
}
