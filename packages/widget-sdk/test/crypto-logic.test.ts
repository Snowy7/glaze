import { describe, expect, it } from "vitest";
import {
  formatChange,
  formatPrice,
  glyphOf,
  nextDelayMs,
  parsePrices,
  pickLayout,
  readCoins,
  symbolOf,
} from "../../../widgets/crypto/logic.js";

describe("crypto formatPrice", () => {
  it("picks decimals by magnitude", () => {
    expect(formatPrice(67234.5, "usd", Infinity, "en-US")).toBe("$67,235");
    expect(formatPrice(162.074, "usd", Infinity, "en-US")).toBe("$162.07");
    expect(formatPrice(0.1834, "usd", Infinity, "en-US")).toBe("$0.1834");
    expect(formatPrice(0.00001234, "usd", Infinity, "en-US")).toBe("$0.000012");
  });

  it("never shows yen decimals from 1 up, and goes compact only when asked and too long", () => {
    expect(formatPrice(10_085_174.6, "jpy", Infinity, "en-US")).toBe("¥10,085,175");
    expect(formatPrice(10_085_174.6, "jpy", 7, "en-US")).toBe("¥10.1M");
    expect(formatPrice(67234.5, "usd", 7, "en-US")).toBe("$67,235"); // fits: stays exact
    expect(formatPrice(0.00001234, "usd", 3, "en-US")).toBe("$0.000012"); // only thousands compact
  });
});

describe("crypto glyphOf", () => {
  it("uses a coin's sign, else the first letter of its ticker", () => {
    expect(glyphOf("bitcoin")).toBe("₿");
    expect(glyphOf("tron")).toBe("T");
    expect(glyphOf("some-new-coin")).toBe("S");
  });
});

describe("crypto formatChange", () => {
  it("signs, rounds and picks a direction", () => {
    expect(formatChange(2.43)).toEqual({ text: "+2.4%", up: true });
    expect(formatChange(-1.82)).toEqual({ text: "−1.8%", up: false });
    expect(formatChange(142.6)).toEqual({ text: "+143%", up: true });
  });

  it("treats a change that rounds to zero as flat and up", () => {
    expect(formatChange(-0.02)).toEqual({ text: "0.0%", up: true });
  });
});

describe("crypto nextDelayMs", () => {
  const min = 60_000;
  it("doubles per failure and caps at 30 minutes", () => {
    expect([0, 1, 2, 3, 4].map((n) => nextDelayMs(5, n) / min)).toEqual([5, 10, 20, 30, 30]);
  });

  it("keeps the user's interval within 2 to 30 minutes", () => {
    expect(nextDelayMs(0, 0)).toBe(2 * min);
    expect(nextDelayMs(90, 0)).toBe(30 * min);
  });
});

describe("crypto settings and replies", () => {
  it("keeps unique, plausible coin ids", () => {
    const raw = [{ id: " Bitcoin " }, { id: "bitcoin" }, { id: "no spaces" }, { id: 5 }, null, { id: "avalanche-2" }];
    expect(readCoins(raw)).toEqual(["bitcoin", "avalanche-2"]);
    expect(readCoins("bitcoin")).toEqual([]);
    expect(readCoins(Array.from({ length: 9 }, (_, i) => ({ id: `coin${i}` })))).toHaveLength(6);
  });

  it("falls back to the id as a label", () => {
    expect(symbolOf("bitcoin")).toBe("BTC");
    expect(symbolOf("some-new-token")).toBe("SOME-NEW-TOKEN");
  });

  it("reads quotes and leaves out coins the API didn't answer for", () => {
    const reply = { bitcoin: { eur: 61000, eur_24h_change: -0.5 }, ethereum: { eur: 2200 }, junk: "x" };
    expect(parsePrices(reply, ["bitcoin", "ethereum", "junk", "missing"], "eur")).toEqual({
      bitcoin: { price: 61000, change: -0.5 },
      ethereum: { price: 2200 },
    });
    expect(parsePrices(null, ["bitcoin"], "eur")).toEqual({});
  });
});

describe("crypto pickLayout", () => {
  const pad = 4;
  it("shows one big coin when tiny", () => {
    expect(pickLayout({ w: 96, h: 96, count: 3, pad })).toMatchObject({ mode: "single", shown: 1 });
  });

  it("stacks rows in a one-cell-wide column", () => {
    expect(pickLayout({ w: 96, h: 200, count: 4, pad })).toMatchObject({ mode: "rows", narrow: true, cols: 1, shown: 3 });
  });

  it("lists as many rows as fit", () => {
    expect(pickLayout({ w: 200, h: 200, count: 3, pad })).toMatchObject({ mode: "rows", cols: 1, shown: 3, footer: true });
    expect(pickLayout({ w: 200, h: 96, count: 3, pad })).toMatchObject({ cols: 1, shown: 2, footer: false });
  });

  it("splits into two columns only when wide and the list is long", () => {
    expect(pickLayout({ w: 408, h: 200, count: 3, pad })).toMatchObject({ cols: 1, shown: 3 });
    expect(pickLayout({ w: 408, h: 200, count: 6, pad })).toMatchObject({ cols: 2, perCol: 3, shown: 6 });
    expect(pickLayout({ w: 408, h: 96, count: 3, pad })).toMatchObject({ cols: 2, perCol: 2, shown: 3 });
  });
});
