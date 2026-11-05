// #2900 JSON.rawJSON / JSON.isRawJSON, #2909 instanceof RHS TypeError.

// --- JSON.rawJSON / JSON.isRawJSON ---
console.log(typeof JSON.rawJSON, typeof JSON.isRawJSON);

const one = JSON.rawJSON("123");
console.log(JSON.isRawJSON(one));
console.log(one.rawJSON);
console.log(JSON.stringify(one));
console.log(JSON.stringify({ a: JSON.rawJSON("123") }));
console.log(JSON.stringify({ a: JSON.rawJSON("123") }) === '{"a":123}');
console.log(JSON.isRawJSON(JSON.rawJSON("1")) === true);
console.log(JSON.isRawJSON({}));
console.log(JSON.isRawJSON({}) === false);
console.log(JSON.stringify([JSON.rawJSON("1"), JSON.rawJSON("true"), JSON.rawJSON("null")]));
console.log(JSON.stringify({ n: JSON.rawJSON("3.14"), s: JSON.rawJSON("\"hi\"") }));

// --- instanceof RHS TypeError (#2909) ---
class Ctor {}
const c = new Ctor();
console.log(c instanceof Ctor);
console.log(new Date() instanceof Date);

function check(label: string, fn: () => unknown): void {
  try {
    const r = fn();
    console.log(label, "ok", r);
  } catch (e) {
    const err = e as Error;
    console.log(label, "throw", err.name, err.message);
  }
}

const five: any = 5;
const u: any = undefined;
const str: any = "x";
const nul: any = null;
check("{} instanceof 5", () => ({} as any) instanceof five);
check("{} instanceof undefined", () => ({} as any) instanceof u);
check("{} instanceof string", () => ({} as any) instanceof str);
check("{} instanceof null", () => ({} as any) instanceof nul);
