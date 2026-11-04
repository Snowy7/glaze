import { Copy, MousePointerClick, Trash2 } from "lucide-react";
import { useState } from "react";

import { setCount } from "../../lib/appearance";
import type { Appearance, PlacedWidget, WidgetInfo, WidgetSetting } from "../../lib/api";
import type { Json } from "../../lib/barsSchema";
import { displayName, firstFree, fits, nextInstanceId, sizeOf, type DesktopGrid, type Placement } from "../../lib/desktopGrid";
import type { LayoutModel } from "../../lib/layout";
import { settingsWithDefaults } from "../../lib/widgetHost";
import { widgetIcon } from "../../lib/widgetIcons";
import { useStudio } from "../../lib/studio";
import { Button, IconButton, InspectorHeader, InspectorSection, NumberField, Prop, Switch } from "../ui";
import { LookFields } from "./LookFields";
import { editableHere, stackedControl, WidgetSettingControl } from "./WidgetSettingControl";

type Props = {
  item: PlacedWidget;
  widget: WidgetInfo | undefined;
  grid: DesktopGrid;
  /** Every widget on this monitor, for room checks. */
  placements: readonly Placement[];
  model: LayoutModel;
  /** The theme's widget look: the layer under the look of all widgets. */
  themeLook: Appearance;
  onBack: () => void;
  /** Opens the widget host's own editor on the desktop (lists, pictures). */
  onEditOnDesktop: () => void;
  onSelect: (instance: string) => void;
};

/** A placed widget: its own options (from its manifest), its size, and, if wanted, a look of its own. */
export function WidgetInspector({ item, widget, grid, placements, model, themeLook, onBack, onEditOnDesktop, onSelect }: Props) {
  const { notify } = useStudio();
  const [ownLook, setOwnLook] = useState(setCount(item.appearance) > 0);
  const size = sizeOf(item, widget);
  const settings = widget?.manifest?.settings ?? [];
  const values = settingsWithDefaults(settings, item.settings);
  const name = widget?.name ?? item.widget;
  const placement: Placement = { instance: item.instance, cell: item.cell ?? [0, 0], size };

  function setSetting(setting: WidgetSetting, value: Json | undefined) {
    const { [setting.key]: _old, ...rest } = item.settings;
    model.put({ instance: item.instance, settings: value === undefined ? rest : { ...rest, [setting.key]: value } }, `Change ${setting.label.toLowerCase()}`);
  }

  function resize(axis: 0 | 1, value: number) {
    const next: [number, number] = axis === 0 ? [value, size[1]] : [size[0], value];
    if (!fits(grid, { ...placement, size: next }, placements)) return notify(`There's no room to make ${name} that big here. Move it first.`);
    model.put({ instance: item.instance, size: next }, "Resize widget");
  }

  function duplicate() {
    const cell = firstFree(grid, size, placements);
    if (!cell) return notify("There's no free space on this display for another one.");
    const instance = nextInstanceId(item.widget, placements.map((p) => p.instance));
    model.put({ instance, widget: item.widget, monitor: item.monitor, cell, size, settings: item.settings, appearance: item.appearance }, `Duplicate ${name}`);
    onSelect(instance);
  }

  const groups = [...new Set(settings.filter(editableHere).map((s) => s.group ?? "Options"))];
  const elsewhere = settings.filter((s) => !editableHere(s));

  return (
    <>
      <InspectorHeader
        icon={widgetIcon(item.widget)}
        title={name}
        detail={`${size[0]}×${size[1]} cells · ${displayName(item.monitor)}`}
        back={{ label: "All widgets", onClick: onBack }}
        actions={
          <>
            <IconButton icon={Copy} variant="ghost" size="sm" label={`Duplicate ${name}`} onClick={duplicate} />
            <IconButton icon={Trash2} variant="ghost" size="sm" label={`Remove ${name}`} onClick={() => model.remove(item)} />
          </>
        }
      />
      {!widget && (
        <InspectorSection>
          <p className="text-[13px] text-ink-3">This widget isn't installed, so it isn't shown. Remove it, or install it again and it comes back here.</p>
        </InspectorSection>
      )}
      {groups.map((group) => (
        <InspectorSection key={group} title={group}>
          {settings
            .filter((s) => editableHere(s) && (s.group ?? "Options") === group)
            .map((setting) => (
              <Prop
                key={setting.key}
                id={`widget.${item.widget}.${setting.key}`}
                label={setting.label}
                stacked={stackedControl(setting)}
                inherited={item.settings[setting.key] === undefined ? "Default" : undefined}
                onReset={() => setSetting(setting, undefined)}
              >
                <WidgetSettingControl setting={setting} value={values[setting.key]} onChange={(value) => setSetting(setting, value)} onEditOnDesktop={onEditOnDesktop} />
              </Prop>
            ))}
        </InspectorSection>
      ))}
      {widget?.resizable && (
        <InspectorSection title="Size">
          <div className="grid grid-cols-2 gap-2">
            <NumberField className="w-full" label="Width in cells" unit="wide" value={size[0]} min={widget.size.min[0]} max={widget.size.max?.[0] ?? grid.columns} onChange={(v) => resize(0, v)} />
            <NumberField className="w-full" label="Height in cells" unit="tall" value={size[1]} min={widget.size.min[1]} max={widget.size.max?.[1] ?? grid.rows} onChange={(v) => resize(1, v)} />
          </div>
        </InspectorSection>
      )}
      <InspectorSection title="Look">
        <Prop id="widget.ownLook" label="Its own look" hint={ownLook ? undefined : "Follows the look of all widgets."}>
          <Switch
            label="Its own look"
            checked={ownLook}
            onChange={(on) => {
              setOwnLook(on);
              if (!on && setCount(item.appearance) > 0) model.put({ instance: item.instance, appearance: {} }, "Use the look of all widgets");
            }}
          />
        </Prop>
      </InspectorSection>
      {ownLook && (
        <LookFields
          target="widgets"
          prefix="widget.look"
          value={item.appearance}
          below={[themeLook, model.layout?.appearance ?? {}]}
          belowLabel="All widgets"
          onChange={(appearance, label) => model.put({ instance: item.instance, appearance }, label)}
        />
      )}
      {elsewhere.length > 0 && (
        <InspectorSection title="On the desktop">
          <p className="text-[12.5px] leading-relaxed text-ink-3">{elsewhere.map((s) => s.label).join(", ")}: set these on the desktop, where you can pick files and edit lists.</p>
          <Button size="sm" icon={MousePointerClick} className="self-start" onClick={onEditOnDesktop}>
            Edit on desktop
          </Button>
        </InspectorSection>
      )}
    </>
  );
}
