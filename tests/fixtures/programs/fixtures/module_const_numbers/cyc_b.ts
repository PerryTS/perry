import { readFlag, readWidth, readBoth, FLAG, WIDTH } from "./cyc_a.ts";

function name(e: any): string { return e && e.constructor ? e.constructor.name : String(e); }
export const early: string[] = [];
try { early.push("call " + readFlag(1)); } catch (e) { early.push("call " + name(e)); }
try { early.push("width " + readWidth()); } catch (e) { early.push("width " + name(e)); }
try { early.push("both " + readBoth()); } catch (e) { early.push("both " + name(e)); }
try { early.push("direct " + (FLAG | 1)); } catch (e) { early.push("direct " + name(e)); }
try { early.push("direct2 " + WIDTH); } catch (e) { early.push("direct2 " + name(e)); }
export const B_SHIFT = 1 << 3;
export function lateB(): string {
  return [readFlag(1), readWidth(), readBoth(), FLAG, WIDTH, FLAG << B_SHIFT, (FLAG & WIDTH) === 0].join(" ");
}
