import path from "node:path";

// #2992 — path.format top-level argument validation + field coercion
console.log("=== format validation ===");
for (const value of [null, undefined, 1, "x"]) {
  try {
    const r = path.format(value as any);
    console.log("ok", JSON.stringify(r));
  } catch (err: any) {
    console.log(err.name, err.message, err.code);
  }
}

console.log("=== format coercion ===");
console.log(JSON.stringify(path.format({ dir: 1, base: "b" } as any)));
console.log(JSON.stringify(path.format({ dir: "/d", base: 1 } as any)));
console.log(JSON.stringify(path.format({ root: 1, name: "n", ext: "e" } as any)));
console.log(JSON.stringify(path.win32.format({ dir: 1, base: "b" } as any)));
console.log(JSON.stringify(path.win32.format({ dir: "/d", base: 1 } as any)));
console.log(JSON.stringify(path.format({ dir: "/d", base: "b" })));
console.log(JSON.stringify(path.format({ root: "/", base: "b" })));
console.log(JSON.stringify(path.format({ name: "file", ext: "txt" })));
console.log(JSON.stringify(path.format({ base: 0 } as any)));
console.log(JSON.stringify(path.win32.format({ dir: "C:\\a\\b", base: "c.js" } as any)));

// #2995 — path.relative argument validation
console.log("=== relative validation ===");
const relCases: any[] = [[1, "x"], ["x", 1], [null, "x"], ["x", undefined]];
for (const args of relCases) {
  try {
    const r = path.relative(args[0], args[1]);
    console.log("ok", JSON.stringify(r));
  } catch (err: any) {
    console.log(err.name, err.message, err.code);
  }
}
console.log("=== relative win32 validation ===");
for (const args of relCases) {
  try {
    const r = path.win32.relative(args[0], args[1]);
    console.log("ok", JSON.stringify(r));
  } catch (err: any) {
    console.log(err.name, err.message, err.code);
  }
}

// #2812 — String.prototype.includes(position)
console.log("=== includes position ===");
const s = "ababa";
console.log(s.includes("a"));
console.log(s.includes("a", 1));
console.log(s.includes("a", 5));
console.log(s.includes("a", -10));
console.log(s.includes("a", Infinity));
console.log(s.includes("a", NaN));
console.log(s.includes("b", 2));
console.log(s.includes("x", 0));
