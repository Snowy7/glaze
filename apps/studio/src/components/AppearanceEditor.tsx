import type { Appearance, SkinKind } from "../lib/api";
import { LookFields } from "./inspectors/LookFields";

type Props = {
  target: SkinKind;
  /** This layer: only the fields the user has set. */
  value: Appearance;
  onChange: (next: Appearance) => void;
  /** The layers under this one, lowest first. */
  below?: readonly Appearance[];
  /** What the layers below are, in words ("the theme"). */
  fallback: string;
  /** The page draws its own preview; kept for callers that still pass it. */
  hidePreview?: boolean;
};

/**
 * The look fields inside a settings card, for the Bars page's Look tab until that page moves to
 * the inspector (docs/studio-ux.md, "Bars"). Everything else uses `LookFields` directly.
 */
export function AppearanceEditor({ target, value, onChange, below = [], fallback }: Props) {
  return (
    <div className="[&_details]:border-0 [&_section]:border-0">
      <LookFields target={target} value={value} below={below} belowLabel={fallback === "the theme" ? "Theme" : "Default"} onChange={(next) => onChange(next)} prefix={`${target}.look`} />
    </div>
  );
}
