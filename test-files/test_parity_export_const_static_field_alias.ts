import { z } from "./fixtures/parity_5891/export_const_static_alias/barrel.ts";
import { string as direct } from "./fixtures/parity_5891/export_const_static_alias/types.ts";
const a = (z as any).string();
if (!a || (a as any).v !== 5) throw new Error("via namespace: " + JSON.stringify(a));
const b = (direct as any)();
if (!b || (b as any).v !== 5) throw new Error("direct import: " + JSON.stringify(b));
console.log("OK");
