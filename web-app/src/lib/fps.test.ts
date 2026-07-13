import { describe, it, expect } from "vitest";
import { emaFps } from "./fps";

describe("emaFps", () => {
  it("cold start (prevEma<=0) snaps to the first instantaneous reading", () => {
    expect(emaFps(0, 1000 / 60)).toBeCloseTo(60, 5);
  });

  it("converges toward a steady frame rate", () => {
    let ema = 0;
    for (let i = 0; i < 300; i++) ema = emaFps(ema, 1000 / 120);
    expect(ema).toBeCloseTo(120, 0);
  });

  it("ignores non-positive dt (returns prev unchanged)", () => {
    expect(emaFps(60, 0)).toBe(60);
    expect(emaFps(60, -5)).toBe(60);
  });
});
