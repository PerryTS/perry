// The inliner refused to inline a function whose body builds a closure over one
// of its PARAMETERS, but not over one of its LOCALS — and a captured-and-mutated
// local is *boxed*.
//
// The closure body is compiled once, from the original function, and reads its
// capture slot as a box pointer (js_closure_get_capture_bits -> js_box_get_bits).
// Cloning the callee's body into a call site re-derived the local there as a
// plain slot, so the call site stored the local's *value* into the capture slot
// instead of a box. The closure then dereferenced that value as a box pointer:
// every read came back `undefined` and every write from inside the closure was
// lost.
//
//     function mk() { let k = 0; return () => { for (const x of [1]) { k = 7; } return k; }; }
//     const g = mk();
//     g();   // undefined, expected 7
//
// It only bit when the enclosing function was actually inlined — calling
// `mk()()` in place happened to work, which is what made it so slippery.

function mkWrite() {
  let k = 0;
  return () => {
    for (const x of [1]) {
      k = 1000 + x;
    }
    return k;
  };
}
const write = mkWrite();
if (write() !== 1001) throw new Error(`captured write in for-of = ${write()}, expected 1001`);

// The write must persist across calls (the box is shared state).
function mkCounter() {
  let n = 0;
  return () => {
    n = n + 1;
    return n;
  };
}
const counter = mkCounter();
if (counter() !== 1 || counter() !== 2 || counter() !== 3) {
  throw new Error("counter closure did not accumulate");
}

// Two closures over the same boxed local must see each other's writes.
function mkPair() {
  let v = "initial";
  return {
    set: (x) => {
      v = x;
    },
    get: () => v,
  };
}
const pair = mkPair();
if (pair.get() !== "initial") throw new Error(`pair.get() = ${pair.get()}, expected "initial"`);
pair.set("updated");
if (pair.get() !== "updated") throw new Error(`pair.get() = ${pair.get()}, expected "updated"`);

// A read-only capture (never boxed) must keep working.
function mkRead() {
  const c = 42;
  return () => c;
}
if (mkRead()() !== 42) throw new Error("read-only capture broke");

// A closure capturing a PARAMETER — the case the inliner already excluded.
function mkParam(p) {
  return () => {
    p = p + 1;
    return p;
  };
}
const fromParam = mkParam(10);
if (fromParam() !== 11 || fromParam() !== 12) throw new Error("parameter capture broke");

console.log("inlined closure captures ok");
