import { MousePointerClick } from "lucide-react";

import type { SelectOption, WidgetSetting } from "../../lib/api";
import { FONT_PRESETS } from "../../lib/appearance";
import type { Json } from "../../lib/barsSchema";
import { Button, ColorField, Combobox, NumberField, Segmented, Select, Slider, Switch, TextInput } from "../ui";
import { PlaceControl } from "./PlaceControl";

const optionOf = (option: SelectOption) => (typeof option === "string" ? { value: option, label: option } : { value: option.value, label: option.label ?? option.value, icon: option.icon });

/** Whether a setting is edited right in the inspector; lists, files and pictures are edited on the desktop. */
export const editableHere = (setting: WidgetSetting) => !["list", "file", "image"].includes(setting.type);

/** Whether a setting's control goes under its label (wide controls) rather than beside it. */
export const stackedControl = (setting: WidgetSetting) =>
  setting.type === "string" || setting.type === "font" || setting.type === "slider" || setting.type === "place" || (setting.type === "select" && setting.options.length > 3);

type Props = {
  setting: WidgetSetting;
  value: Json | undefined;
  /** The widget's id: a place search runs the widget's own plugin. */
  widget: string;
  onChange: (value: Json) => void;
  onEditOnDesktop: () => void;
};

/** The control for one of a widget's own settings (packages/widget-sdk, `SettingDef`). */
export function WidgetSettingControl({ setting, value, widget, onChange, onEditOnDesktop }: Props) {
  switch (setting.type) {
    case "boolean":
      return <Switch label={setting.label} checked={value === true} onChange={onChange} />;
    case "select": {
      const options = setting.options.map(optionOf);
      const current = typeof value === "string" ? value : (options[0]?.value ?? "");
      return options.length <= 3 && options.every((o) => o.label.length <= 12) ? (
        <Segmented<string> label={setting.label} hideLabel size="sm" value={current} options={options.map((o) => ({ value: o.value, label: o.label }))} onChange={onChange} />
      ) : (
        <Select<string> label={setting.label} className="w-full" value={current} options={options.map((o) => ({ value: o.value, label: o.icon ? `${o.icon}  ${o.label}` : o.label }))} onChange={onChange} />
      );
    }
    case "slider":
      return <Slider className="w-full" label={setting.label} value={typeof value === "number" ? value : setting.min} min={setting.min} max={setting.max} step={setting.step ?? 1} format={(v) => `${v}${setting.unit ?? ""}`} parse={Number.parseFloat} onChange={onChange} />;
    case "number":
      return <NumberField label={setting.label} value={typeof value === "number" ? value : (setting.min ?? 0)} min={setting.min} max={setting.max} step={setting.step ?? 1} onChange={onChange} />;
    case "string":
      return <TextInput compact aria-label={setting.label} className="w-full" value={typeof value === "string" ? value : ""} onChange={(e) => onChange(e.currentTarget.value)} />;
    case "font":
      return <Combobox label={setting.label} className="w-full" value={typeof value === "string" ? value : ""} placeholder="The widget's own" options={FONT_PRESETS.map((p) => ({ value: p.value, label: p.label }))} onChange={onChange} />;
    case "color":
      return <ColorField label={setting.label} value={typeof value === "string" ? value.slice(0, 7) : "#ffffff"} onChange={onChange} />;
    case "place":
      return <PlaceControl setting={setting} value={value} widget={widget} onChange={onChange} />;
    case "list":
    case "file":
    case "image":
      return (
        <Button size="sm" icon={MousePointerClick} onClick={onEditOnDesktop}>
          Edit on desktop
        </Button>
      );
  }
}
