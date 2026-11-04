import type { Desktop } from "../../lib/desktop";
import { ThemePreview } from "../ThemeCard";
import { BarsLayer, type BarPick, type ModuleMove } from "./BarsLayer";
import { displayTools } from "./DesktopScene";
import { Stage, WallpaperLayer } from "./Stage";
import type { BarsEditor } from "../../lib/barsContext";

/**
 * The Bars page's stage: your wallpaper and the bars being edited, live, and nothing else (no
 * widgets). The screen narrows until the bars draw at READABLE (they keep their real size, only
 * the room between their modules shrinks), so a bar and its modules can be read, picked and dragged
 * in any window; it stays tall enough for a dock on a side edge.
 */
const READABLE = { scale: 0.66, minWidth: 1000, minHeight: 800 };

type Props = {
  desktop: Desktop;
  /** The bars being edited (the live document, not the file as saved). */
  editor: BarsEditor | null;
  /** The wallpaper as a data URL, for the bars' glass. */
  wallpaper: string | null;
  selection: BarPick | null;
  onSelect: (pick: BarPick | null) => void;
  onMove: (move: ModuleMove) => void;
};

export function BarsScene({ desktop, editor, wallpaper, selection, onSelect, onMove }: Props) {
  const theme = desktop.active;
  // Until the editor has loaded, the bars as saved.
  const bars = editor ? { doc: editor.doc, config: editor.resolved, catalog: editor.catalog } : desktop.bars;
  return (
    <Stage label="Your bars" screen={desktop.grid.screen} readable={READABLE} onBackground={() => onSelect(null)} tools={displayTools(desktop, () => onSelect(null))}>
      <WallpaperLayer selected={false}>
        {theme ? <ThemePreview name={theme.name} kind={theme.kind} preview={theme.preview} /> : <div className="size-full bg-[#1b1d22]" />}
      </WallpaperLayer>
      {bars && (
        <BarsLayer
          doc={bars.doc}
          config={bars.config}
          catalog={bars.catalog}
          wallpaper={wallpaper}
          mode={editor ? "modules" : "show"}
          selection={selection}
          onSelect={onSelect}
          onMove={onMove}
        />
      )}
    </Stage>
  );
}
