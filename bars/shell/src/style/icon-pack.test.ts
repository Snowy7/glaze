import { describe, expect, it } from "vitest";
import type { IconPack } from "../protocol";
import { iconUrl, matchApp, packGlyphCss } from "./icon-pack";
import { moduleCss } from "./modules";

const pack: IconPack = {
  name: "Test",
  mono: true,
  missing: "app.svg",
  base: "https://packs.bars.mypaper/test/",
  entries: [
    { umid: "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", icon: "terminal.svg" },
    { path: "C:\\Program Files\\Code\\Code.exe", icon: "code-full.svg" },
    { path: "code.exe", icon: "code.svg", mono: false },
    { path: "notepad.exe", icon: "notepad.svg" },
    { extension: "txt", icon: "text.svg" },
    { key: "wifi-3", icon: "wifi 3.svg" },
    { key: "start", icon: "start.svg" },
  ],
};

describe("matchApp", () => {
  it("prefers the AppUserModelID, then a full path, then a file name", () => {
    expect(matchApp(pack, { aumid: "microsoft.windowsterminal_8wekyb3d8bbwe!app", exe: "c:\\x\\wt.exe" })?.icon).toBe(
      "terminal.svg",
    );
    expect(matchApp(pack, { exe: "c:\\program files\\code\\code.exe" })?.icon).toBe("code-full.svg");
    expect(matchApp(pack, { exe: "d:\\portable\\code.exe" })?.icon).toBe("code.svg");
    expect(matchApp(pack, { path: "C:\\Windows\\notepad.exe" })?.icon).toBe("notepad.svg");
  });

  it("falls back to the extension, then the pack's missing icon", () => {
    expect(matchApp(pack, { path: "C:\\notes\\todo.TXT" })?.icon).toBe("text.svg");
    expect(matchApp(pack, { exe: "c:\\x\\other.exe" })?.icon).toBe("app.svg");
    expect(matchApp({ ...pack, missing: undefined }, { exe: "c:\\x\\other.exe" })).toBeUndefined();
  });

  it("serves icons from the pack's base, escaped", () => {
    expect(iconUrl(pack, { key: "wifi-3", icon: "wifi 3.svg" })).toBe("https://packs.bars.mypaper/test/wifi%203.svg");
  });
});

describe("generated CSS", () => {
  it("swaps glyphs by key, as masks for mono packs", () => {
    const css = packGlyphCss(pack);
    expect(css).toContain('.glyph:has(> svg[data-glyph="wifi-3"])');
    expect(css).toContain("mask: url(\"https://packs.bars.mypaper/test/wifi%203.svg\")");
    expect(css).not.toContain("notepad");
  });

  it("styles modules and swaps their icon for a pack key or a served file", () => {
    const css = moduleCss(
      {
        clock: { click: {}, style: { color: "accent", padding: 12, weight: 600 } },
        quick: { click: {}, style: {}, icon: "start" },
        power: { click: {}, style: { background: "#ff000033" }, icon: "C:\\icons\\power.png" },
        "custom:bad\"name": { click: {}, style: { color: "#fff" } },
      },
      pack,
      { "C:\\icons\\power.png": "https://assets.bars.mypaper/1.png" },
    );
    expect(css).toContain('html .mod[data-module="clock"] { --i-fg: var(--accent); --i-fg-hover: var(--accent); color: var(--accent); padding-inline: 12px; font-weight: 600; }');
    expect(css).toContain('.mod[data-module="quick"] .glyph:not(.small) { background-color: currentColor;');
    expect(css).toContain('background: url("https://assets.bars.mypaper/1.png")');
    expect(css).toContain("--i-bgc: #ff000033");
    expect(css).not.toContain("bad");
  });
});
