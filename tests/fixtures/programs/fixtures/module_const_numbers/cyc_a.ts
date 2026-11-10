// N6 fixture: cyc_a imports cyc_b, which imports cyc_a back, so cyc_b's body
// runs first and reaches cyc_a's functions while FLAG and WIDTH are in their
// dead zone.
import { early, lateB } from "./cyc_b.ts";

export const FLAG = 1 << 12;
export const WIDTH = FLAG >> 4;
export function readFlag(v: number): number { return v | FLAG; }
export function readWidth(): number { return WIDTH + 1; }
export function readBoth(): number { return (FLAG & 0xff00) + WIDTH; }
export { early, lateB };
