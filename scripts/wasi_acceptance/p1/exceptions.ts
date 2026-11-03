// try/catch/finally, and exceptions thrown across calls, on the core-module
// target (wasm exception handling must be enabled by the host).
function inner(n: number): number {
  if (n === 0) throw new Error("bottom");
  return inner(n - 1) + 1;
}
function middle(): number {
  try {
    return inner(3);
  } finally {
    console.log("middle finally");
  }
}
function rethrow(): void {
  try {
    middle();
  } catch (error) {
    console.log("rethrowing", (error as Error).message);
    throw new TypeError("wrapped");
  }
}
try {
  rethrow();
} catch (error) {
  console.log((error as Error).name, (error as Error).message);
} finally {
  console.log("outer finally");
}
const results: string[] = [];
for (let i = 0; i < 3; i++) {
  try {
    if (i === 1) throw i;
    results.push("ok" + i);
  } catch (value) {
    results.push("caught" + value);
  } finally {
    results.push("done" + i);
  }
}
console.log(results.join(","));
console.log("continued");
