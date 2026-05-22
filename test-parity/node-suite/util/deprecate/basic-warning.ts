import { deprecate } from "node:util";

const fn = deprecate((x: number) => x + 1, "deprecated fn", "DEP_PERRY_TEST");
console.log("call1:", fn(1));
console.log("call2:", fn(2));
try { deprecate(() => 1, "msg", "bad code with spaces"); console.log("bad code no throw"); } catch (err: any) { console.log("bad code:", err?.name, err?.code || "no-code"); }
