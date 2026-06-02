import { fromB } from "./dynamic_import_cycle_b.ts";

export const fromA: string = "a-export";
export const viaB: string = fromB;
