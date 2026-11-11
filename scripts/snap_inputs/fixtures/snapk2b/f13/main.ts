import { e } from "./m.ts";
gc();
// Reading .stack names the capturing function, so its body is JS text.
console.log("K2B_F13", e.message, String(e.stack).includes("make"));
