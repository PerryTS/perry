import { table, words } from "./dep_a";
export function makeCounter(): () => number {
  let n = 0;
  return () => ++n;
}
export const counters = [makeCounter(), makeCounter()];
export const index = new Map<string | number, unknown>();
for (let i = 0; i < table.length; i++) {
  index.set(table[i].name, table[i]);
  index.set(i, words[i % words.length]);
}
export const state = { inits: 0 };
