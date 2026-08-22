import { z } from "./fixtures/parity_5891/namespace_reexport/barrel.ts";
if (typeof z !== "object") throw new Error(`typeof z: ${typeof z}`);
if (typeof (z as any).object !== "function" || typeof (z as any).coerce !== "object") throw new Error("members");
if ((z as any).coerce.number() !== 42 || (z as any).object() !== "obj") throw new Error("calls");
console.log("OK");
