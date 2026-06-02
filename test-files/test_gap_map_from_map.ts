// #33/#321: `new Map(existingMap)` must copy entries. A Map is itself an
// iterable of [key, value] pairs, so cloning one with `new Map(other)` should
// reproduce every entry. Perry previously treated the source Map's pointer as
// an ArrayHeader (the two share a `(u32, u32)` prefix that means different
// things) and produced an EMPTY map — which broke effect's
// `FiberRefs.updateAs` (`new Map(self.locals)` dropped every fiber-ref except
// the one being set), corrupting the `currentScheduler` override and making
// `Effect.runSync` of a `Layer`/`Context`-provided effect throw
// "Fiber #0 cannot be resolved synchronously".
// Run: node --experimental-strip-types test_gap_map_from_map.ts

// --- new Map(existingMap) copies all entries ---
const src = new Map<string, number>();
src.set("x", 1);
src.set("y", 2);
src.set("z", 3);

const copy = new Map(src);
console.log("copy.size:", copy.size); // 3
console.log("copy.x:", copy.get("x")); // 1
console.log("copy.y:", copy.get("y")); // 2
console.log("copy.z:", copy.get("z")); // 3
console.log("copy.has(x):", copy.has("x")); // true

// The copy is independent: mutating it does not touch the source.
copy.set("x", 99);
console.log("after copy.set x=99 -> src.x:", src.get("x")); // 1
console.log("after copy.set x=99 -> copy.x:", copy.get("x")); // 99

// --- empty source ---
const empty = new Map<string, number>();
const emptyCopy = new Map(empty);
console.log("emptyCopy.size:", emptyCopy.size); // 0

// --- numeric keys survive the copy ---
const numKeys = new Map<number, string>();
numKeys.set(1, "one");
numKeys.set(2, "two");
const numCopy = new Map(numKeys);
console.log("numCopy.size:", numCopy.size); // 2
console.log("numCopy.get(1):", numCopy.get(1)); // one
console.log("numCopy.get(2):", numCopy.get(2)); // two

// --- the array-of-entries form still works (regression guard) ---
const fromEntries = new Map([
  ["a", 10],
  ["b", 20],
]);
console.log("fromEntries.size:", fromEntries.size); // 2
console.log("fromEntries.a:", fromEntries.get("a")); // 10

// --- new Map([...map]) (spread) still works ---
const fromSpread = new Map([...src]);
console.log("fromSpread.size:", fromSpread.size); // 3
console.log("fromSpread.y:", fromSpread.get("y")); // 2
