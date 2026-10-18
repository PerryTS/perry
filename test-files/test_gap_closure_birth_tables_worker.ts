// A program that uses worker_threads owns its class keys per agent, so a
// literal of arrow getters born from a descriptor (`closure_births`) names
// this agent's key and ShapeId slots, not link-time addresses (#10399).
const { isMainThread } = require("worker_threads");

function exportsOf(seed: number) {
  let v0 = seed, v1 = seed + 1, v2 = seed + 2, v3 = seed + 3, v4 = seed + 4;
  const table = {
    g0: () => v0, g1: () => v1, g2: () => v2, g3: () => v3, g4: () => v4,
    g5: () => v0 + v1, g6: () => v2 * v3, g7: () => v4 - v0, g8: () => seed,
    bump: () => { v0 += 10; v4 += 10; },
  };
  return table;
}

const a = exportsOf(1);
const b = exportsOf(100);
a.bump();
console.log(isMainThread, a.g0(), a.g4(), a.g5(), a.g7(), a.g8(), b.g0(), b.g6(), a.g1 === b.g1);
console.log(Object.keys(a).join(","));
