import { read } from "./b.ts";

console.log("early", read());
export const K = 3;
export let L: number | undefined;
export const { d1, d2 } = { d1: 1, d2: 2 };
export const fe = function fe(): string {
  return "fe";
};
const hidden = 4;
export { hidden as renamed };
L = 5;
console.log("late", read());
