// #11621: pair conversion must unwind to JS catches and release temporary roots.
declare const gc: undefined | (() => void);
function collect() { if (typeof gc === "function") gc(); }
function pipe(pair: any, options?: any): any {
  const source: any = new ReadableStream({ start(c) { c.close(); } });
  return source.pipeThrough(pair, options);
}
for (const value of [NaN, Infinity, -Infinity, 1048576.5]) {
  try { pipe(value); console.log("number accepted"); }
  catch (error) { console.log("number", error instanceof TypeError); }
  collect();
}
for (const value of [{}, { readable: 123 }, { writable: 123 }]) {
  try { pipe(value); console.log("pair accepted"); }
  catch (error) { console.log("pair", error instanceof TypeError); }
  collect();
}
const transform = new TransformStream();
for (const value of [
  { readable: 123, writable: transform.writable },
  { readable: transform.readable, writable: 123 },
]) {
  try { pipe(value); console.log("endpoint accepted"); }
  catch (error) { console.log("endpoint", error instanceof TypeError); }
  collect();
}
for (const field of ["readable", "writable"]) {
  const value = {
    get readable() { if (field === "readable") throw new Error("readable getter"); return transform.readable; },
    get writable() { throw new Error("writable getter"); },
  };
  try { pipe(value); console.log("getter accepted"); }
  catch (error) { console.log("getter", (error as Error).message); }
  collect();
}
try { pipe(new TransformStream(), { get preventClose() { throw new Error("options getter"); } }); console.log("options accepted"); }
catch (error) { console.log("options", (error as Error).message); }
collect();
console.log("after catches", new Uint8Array([7, 8]).length);
