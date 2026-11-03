import { describe, expect, it } from "vitest";
import { captionOf, createRotation, imageUrl, readImages } from "../../../widgets/photo/rotation.js";

/** A small deterministic generator, so shuffles are reproducible. */
function seeded(seed: number) {
  let s = seed;
  return () => {
    s = (s * 1664525 + 1013904223) % 2 ** 32;
    return s / 2 ** 32;
  };
}

const take = (r: { next(): number }, n: number) => Array.from({ length: n }, () => r.next());

describe("photo createRotation", () => {
  it("goes through the list in order and wraps", () => {
    expect(take(createRotation(3, false), 7)).toEqual([0, 1, 2, 0, 1, 2, 0]);
  });

  it("shuffles in rounds: every image once per round, never the same twice in a row", () => {
    for (const seed of [1, 2, 3, 4, 5, 6, 7, 8]) {
      const order = take(createRotation(4, true, seeded(seed)), 40);
      for (let round = 0; round < 10; round++) {
        expect([...order.slice(round * 4, round * 4 + 4)].sort()).toEqual([0, 1, 2, 3]);
      }
      order.slice(1).forEach((n, i) => expect(n).not.toBe(order[i]));
    }
  });

  it("peek tells what next returns", () => {
    const r = createRotation(5, true, seeded(9));
    for (let i = 0; i < 12; i++) {
      const upcoming = r.peek();
      expect(r.next()).toBe(upcoming);
    }
  });

  it("copes with one image", () => {
    expect(take(createRotation(1, true), 3)).toEqual([0, 0, 0]);
  });
});

describe("photo settings", () => {
  it("keeps unique image files and drops everything else", () => {
    const raw = [{ file: " a.svg " }, { file: "a.svg" }, { file: "../secret.png" }, { file: "notes.txt" }, { file: "b c.JPG" }, { file: 3 }, null];
    expect(readImages(raw)).toEqual(["a.svg", "b c.JPG"]);
    expect(readImages("a.svg")).toEqual([]);
    expect(readImages(Array.from({ length: 30 }, (_, i) => ({ file: `p${i}.png` })))).toHaveLength(24);
  });

  it("names the caption and the url", () => {
    expect(captionOf("misty-lake_01.jpg")).toBe("misty lake 01");
    expect(imageUrl("my photo #1.png")).toBe("images/my%20photo%20%231.png");
  });
});
