// #11826 fixture: the module an import cycle re-enters before its body ran.
import { fromB } from "./cyc_b.ts";
export const CA: any = { v: 1 };
export let LA = 2;
export function readCA() { return CA.v; }
export function readLA() { return LA; }
console.log("a body", fromB, readCA(), readLA());
