import { describe, expect, it } from "vitest";
import { parse, parseInline, toggleTask } from "../../../widgets/notes/md.js";

describe("notes markdown-lite", () => {
  it("parses inline styles and leaves unmatched markers alone", () => {
    expect(parseInline("a **b** `c` ~~d~~ *e* _f_ g")).toEqual([
      { kind: "text", text: "a " },
      { kind: "bold", text: "b" },
      { kind: "text", text: " " },
      { kind: "code", text: "c" },
      { kind: "text", text: " " },
      { kind: "strike", text: "d" },
      { kind: "text", text: " " },
      { kind: "italic", text: "e" },
      { kind: "text", text: " " },
      { kind: "italic", text: "f" },
      { kind: "text", text: " g" },
    ]);
    expect(parseInline("2 * 3 and snake_case_name")).toEqual([{ kind: "text", text: "2 * 3 and snake_case_name" }]);
  });

  it("classifies one block per line and keeps the source line", () => {
    const blocks = parse("# Title\n- [x] done\n- [ ] todo\n* item\n> quote\n---\n\nplain");
    expect(blocks.map((b) => [b.line, b.kind])).toEqual([
      [0, "heading"],
      [1, "task"],
      [2, "task"],
      [3, "bullet"],
      [4, "quote"],
      [5, "rule"],
      [6, "blank"],
      [7, "paragraph"],
    ]);
    expect(blocks[1]).toMatchObject({ checked: true });
    expect(blocks[2]).toMatchObject({ checked: false });
  });

  it("never produces markup: html stays plain text", () => {
    const [block] = parse("<img src=x onerror=alert(1)>");
    expect(block).toMatchObject({ kind: "paragraph", inline: [{ kind: "text", text: "<img src=x onerror=alert(1)>" }] });
  });

  it("toggles only the task on the given line, both ways", () => {
    const text = "- [ ] a\n- [x] b\nplain";
    expect(toggleTask(text, 0)).toBe("- [x] a\n- [x] b\nplain");
    expect(toggleTask(text, 1)).toBe("- [ ] a\n- [ ] b\nplain");
    expect(toggleTask(text, 2)).toBe(text);
    expect(toggleTask(text, 9)).toBe(text);
  });
});
