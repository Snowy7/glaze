import { describe, expect, it } from "vitest";
import { THUMB_MAX, thumbSizes } from "./preview";

describe("thumbSizes", () => {
  it("keeps each window's aspect inside the largest thumbnail", () => {
    const [wide, tall, tiny] = thumbSizes([
      { width: 2560, height: 1440 },
      { width: 800, height: 1600 },
      { width: 10, height: 10 },
    ]);
    expect(wide).toEqual({ w: THUMB_MAX.w, h: Math.round(1440 * (THUMB_MAX.w / 2560)) });
    expect(tall?.h).toBe(THUMB_MAX.h);
    expect(tall?.w).toBe(Math.max(48, Math.round(800 * (THUMB_MAX.h / 1600))));
    expect(tiny?.w).toBeLessThanOrEqual(THUMB_MAX.w);
    expect(tiny?.h).toBeLessThanOrEqual(THUMB_MAX.h);
  });
});
