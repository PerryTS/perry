function override() { try { throw new Error("discarded"); } finally { return 7; } }
console.log("override", override());
try {
  try { throw new Error("original"); }
  catch (e) { throw e; }
  finally { console.log("nested finally"); }
} catch (e: any) { console.log("rethrow", e.message); }
try { JSON.parse("{"); } catch (e) { console.log("runtime", e instanceof SyntaxError); }
let closed = 0;
function sequence(throwOnClose: boolean) {
  return { [Symbol.iterator]() { return {
    next() { return { done: false, value: 3 }; },
    return() { closed++; if (throwOnClose) throw new Error("iterator return"); return { done: true }; }
  }; } };
}
for (const x of sequence(false)) { console.log("iterator", x); break; }
try { for (const x of sequence(true)) { break; } }
catch (e: any) { console.log(e.message); }
console.log("closed", closed);
// Generic runtime callback and a throwing closure with native-argument ABI.
try { [1, 2].map((x) => { if (x === 2) throw new Error("map callback"); return x; }); }
catch (e: any) { console.log(e.message); }
function callIt(f: (x: number) => number) { return f(11); }
try { callIt((x) => { throw new Error("indirect " + x); }); }
catch (e: any) { console.log(e.message); }
async function rejectAfterResume() {
  await Promise.resolve(1);
  throw new Error("resumed rejection");
}
async function run() {
  try { await rejectAfterResume(); }
  catch (e: any) { console.log(e.message); }
  await Promise.resolve().then(() => { throw new Error("then callback"); })
    .catch((e) => console.log(e.message));
  console.log("async continued");
}
run();
