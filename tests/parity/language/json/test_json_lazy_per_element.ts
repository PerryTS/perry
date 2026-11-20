// Per-element sparse materialization edge-case coverage. Must match
// Node byte-for-byte under direct (default) and lazy
// (`PERRY_JSON_TAPE=1` env-var) paths.

interface Rec { id: number; name: string; nested: { x: number }; tags: string[]; }

const blob = JSON.stringify([
  { id: 0, name: "a", nested: { x: 10 }, tags: ["t0", "z0"] },
  { id: 1, name: "b", nested: { x: 11 }, tags: ["t1", "z1"] },
  { id: 2, name: "c", nested: { x: 12 }, tags: ["t2", "z2"] },
  { id: 3, name: "d", nested: { x: 13 }, tags: ["t3", "z3"] },
  { id: 4, name: "e", nested: { x: 14 }, tags: ["t4", "z4"] },
]);

const parsed = JSON.parse(blob) as Rec[];

// Access — in-bounds.
console.log("len:" + parsed.length);
console.log("[0].id:" + parsed[0].id);
console.log("[4].id:" + parsed[4].id);
console.log("[2].id:" + parsed[2].id);

// Access — field chains.
console.log("[0].name:" + parsed[0].name);
console.log("[1].nested.x:" + parsed[1].nested.x);

// Access — 2D index chain.
console.log("[3].tags[0]:" + parsed[3].tags[0]);
console.log("[3].tags[1]:" + parsed[3].tags[1]);

// Out-of-bounds — undefined.
console.log("[5]:" + parsed[5]);
console.log("[100]:" + parsed[100]);

// Identity — cache must preserve object identity.
const ref0 = parsed[0];
const ref0Again = parsed[0];
console.log("id0===id0:" + (ref0 === ref0Again));
const ref4 = parsed[4];
const ref4Again = parsed[4];
console.log("id4===id4:" + (ref4 === ref4Again));
console.log("id0!==id4:" + (ref0 !== ref4));

// Mutation through cached element — the cache holds a pointer to
// the real object, so `cached.field = v` mutates the object; a
// second `parsed[0]` must see the mutation.
parsed[0].name = "mutated";
console.log("[0].name-after-mutate:" + parsed[0].name);

// Identity survives mutation.
console.log("id0===id0-post-mutate:" + (parsed[0] === ref0));

// Iteration — touches every element; per-element lookup is O(i)
// so the total is O(n²) without the future iteration heuristic.
// For len=5 this is trivial; this test just verifies correctness.
let sum = 0;
for (let i = 0; i < parsed.length; i++) {
  sum += parsed[i].id;
}
console.log("iter-sum:" + sum);

// Stringify after index access — materialized bitmap has bits set,
// must produce byte-correct output matching Node's stringify.
const out = JSON.stringify(parsed);
console.log("stringify-len:" + out.length);
console.log("stringify-has-mutated:" + (out.indexOf("mutated") >= 0));
