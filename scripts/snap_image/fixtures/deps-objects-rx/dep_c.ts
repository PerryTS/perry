import { table } from "./dep_a";
import { state } from "./dep_b";
state.inits++;
export const sums = table.map((t) => t.id * 2);
export const rx = /a(b+)c/g;
