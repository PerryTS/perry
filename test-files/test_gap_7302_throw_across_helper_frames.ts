// Throws that cross runtime Rust helper frames (the Phase-2 question, on the
// real runtime): JSON.parse's parser, a throwing getter reached through the
// property-resolution helper, a throwing toString reached through
// string-coercion, and Array.prototype.map's callback trampoline.
try {
  JSON.parse("{nope");
} catch (e) {
  console.log("json:", (e as Error).constructor.name);
}

const obj = {
  get boom(): number {
    throw new Error("getter-threw");
  },
};
try {
  console.log((obj as any).boom);
} catch (e) {
  console.log("getter:", (e as Error).message);
}

const weird = {
  toString(): string {
    throw new Error("tostring-threw");
  },
};
try {
  console.log("x" + (weird as any));
} catch (e) {
  console.log("tostring:", (e as Error).message);
}

try {
  [1, 2, 3].map((v) => {
    if (v === 2) throw new Error("map-cb");
    return v;
  });
} catch (e) {
  console.log("map:", (e as Error).message);
}

// Deep recursion inside try (shadow-stack savepoint restore across many
// unwound generated frames).
function deep(n: number): number {
  if (n === 0) throw new Error("bottom");
  return deep(n - 1) + 1;
}
try {
  deep(500);
} catch (e) {
  console.log("deep:", (e as Error).message);
}
console.log("done");
