// @ts-check
// Markdown-lite for the notes widget: pure functions, no DOM. The widget builds DOM nodes from
// these blocks (never innerHTML), so note text can't inject markup.
//
// Block syntax (one block per source line): `# ` `## ` `### ` headings, `- ` or `* ` bullets,
// `- [ ] ` / `- [x] ` tasks, `> ` quotes, `---` rules. Inline: **bold**, *italic* or _italic_,
// `code`, ~~strike~~.

/** @typedef {{ kind: "text" | "bold" | "italic" | "code" | "strike", text: string }} Inline */
/**
 * `line` is the index in the source, so a click on a task can edit that line.
 * @typedef {{ line: number } & (
 *   | { kind: "heading", level: 1 | 2 | 3, inline: Inline[] }
 *   | { kind: "bullet", inline: Inline[] }
 *   | { kind: "task", checked: boolean, inline: Inline[] }
 *   | { kind: "quote", inline: Inline[] }
 *   | { kind: "paragraph", inline: Inline[] }
 *   | { kind: "rule" }
 *   | { kind: "blank" }
 * )} Block
 */

const INLINE = /\*\*(.+?)\*\*|`(.+?)`|~~(.+?)~~|\*(.+?)\*|(?<![A-Za-z0-9])_(.+?)_(?![A-Za-z0-9])/g;

/**
 * @param {string} source
 * @returns {Inline[]}
 */
export function parseInline(source) {
  /** @type {Inline[]} */
  const out = [];
  let last = 0;
  for (const m of source.matchAll(INLINE)) {
    if (m.index > last) out.push({ kind: "text", text: source.slice(last, m.index) });
    const [, bold, code, strike, star, under] = m;
    if (bold !== undefined) out.push({ kind: "bold", text: bold });
    else if (code !== undefined) out.push({ kind: "code", text: code });
    else if (strike !== undefined) out.push({ kind: "strike", text: strike });
    else out.push({ kind: "italic", text: star ?? under ?? "" });
    last = m.index + m[0].length;
  }
  if (last < source.length) out.push({ kind: "text", text: source.slice(last) });
  return out;
}

/**
 * @param {string} text
 * @returns {Block[]}
 */
export function parse(text) {
  return text.split("\n").map((raw, line) => {
    const s = raw.replace(/\s+$/, "");
    if (s === "") return { line, kind: "blank" };
    if (/^(-{3,}|\*{3,})$/.test(s)) return { line, kind: "rule" };

    const heading = /^(#{1,3}) +(.*)$/.exec(s);
    if (heading) {
      const level = /** @type {1 | 2 | 3} */ ((heading[1] ?? "#").length);
      return { line, kind: "heading", level, inline: parseInline(heading[2] ?? "") };
    }
    const task = /^\s*[-*] \[([ xX])\] +(.*)$/.exec(s);
    if (task) {
      return { line, kind: "task", checked: task[1] !== " ", inline: parseInline(task[2] ?? "") };
    }
    const bullet = /^\s*[-*] +(.*)$/.exec(s);
    if (bullet) return { line, kind: "bullet", inline: parseInline(bullet[1] ?? "") };
    const quote = /^> ?(.*)$/.exec(s);
    if (quote) return { line, kind: "quote", inline: parseInline(quote[1] ?? "") };
    return { line, kind: "paragraph", inline: parseInline(s) };
  });
}

/**
 * Flips the task checkbox on a source line. Other lines are returned unchanged.
 * @param {string} text
 * @param {number} line
 * @returns {string}
 */
export function toggleTask(text, line) {
  const lines = text.split("\n");
  const current = lines[line];
  if (current === undefined) return text;
  lines[line] = current.replace(/^(\s*[-*] \[)([ xX])(\])/, (_, a, box, c) => `${a}${box === " " ? "x" : " "}${c}`);
  return lines.join("\n");
}
