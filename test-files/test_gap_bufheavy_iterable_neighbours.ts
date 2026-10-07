// Positive iterable neighbours for the async zlib provider regression.
function argumentsValue(..._args: any[]): any { return arguments; }
function custom() {
  return {
    [Symbol.iterator]() {
      let at = 0;
      return { next() { return at < 3 ? { value: ++at, done: false } : { done: true }; } };
    }
  };
}
function make(kind: string): any {
  if (kind === "array") return [1, 2, 3];
  if (kind === "buffer") return Buffer.from([1, 2, 3]);
  if (kind === "uint8") return new Uint8Array([1, 2, 3]);
  if (kind === "uint16") return new Uint16Array([1, 2, 3]);
  if (kind === "map") return new Map([["a", 1], ["b", 2]]).entries();
  if (kind === "set") return new Set([1, 2, 3]).values();
  if (kind === "arguments") return argumentsValue(1, 2, 3);
  if (kind === "string") return "a😀b";
  if (kind === "custom") return custom();
  throw new Error("unknown iterable kind");
}
for (const kind of ["array", "buffer", "uint8", "uint16", "map", "set", "arguments", "string", "custom"]) {
  try {
    const a: any[] = [];
    for (const v of make(kind)) a.push(v);
    console.log(kind, "for-of", JSON.stringify(a));
  } catch (e) { console.log(kind, "for-of", e instanceof TypeError); }
  try { console.log(kind, "spread", JSON.stringify([...make(kind)])); }
  catch (e) { console.log(kind, "spread", e instanceof TypeError); }
  try { const [a, ...b] = make(kind); console.log(kind, "binding", JSON.stringify([a, b])); }
  catch (e) { console.log(kind, "binding", e instanceof TypeError); }
  try { console.log(kind, "from", JSON.stringify(Array.from(make(kind)))); }
  catch (e) { console.log(kind, "from", e instanceof TypeError); }
}
async function drain(value: any) {
  const a: any[] = [];
  for await (const v of value) a.push(v);
  return a;
}
for (const kind of ["array", "buffer", "uint8", "uint16", "map", "set", "arguments", "string", "custom"]) {
  try { console.log(kind, "await", JSON.stringify(await drain(make(kind)))); }
  catch (e) { console.log(kind, "await", e instanceof TypeError); }
}
