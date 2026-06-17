// Regression for #5136: the interprocedural deforestation pass
// (crates/perry-transform/src/deforest) rewrites a "producer" function
// — `const out = []; ...push...; return out;` — to take a trailing
// accumulator parameter, and rewrites its call sites to pass one. The
// call-site rewriter never descended into closure bodies, so when the
// producer's only caller lived inside a closure returned from a
// factory, the producer's signature gained the extra param while the
// in-closure call still passed the original arity. That arity mismatch
// lowered to a garbage accumulator pointer and a SIGSEGV (exit 139).
//
// Surfaced in the wild as `uuid`'s `v5()` crashing under
// `perry.compilePackages`: its `stringToBytes` producer is called from
// the `generateUUID` closure that `v35()` returns.
//
// This mirrors that shape with no dependencies. The fix bails on
// deforesting any producer referenced inside a closure, so the program
// runs correctly. Must print "ok" and exit 0.

function stringToBytes(str: string): number[] {
  const bytes: number[] = [];
  for (let i = 0; i < str.length; ++i) {
    bytes.push(str.charCodeAt(i));
  }
  return bytes;
}

function makeEncoder() {
  function encode(value: string): number {
    const v = stringToBytes(value);
    let sum = 0;
    for (let i = 0; i < v.length; ++i) sum += v[i];
    return sum;
  }
  return encode;
}

const encode = makeEncoder();
// "perry" => 112+101+114+114+121 = 562
const sum = encode("perry");
if (sum !== 562) {
  console.log("FAIL: expected 562, got", sum);
  throw new Error("deforest closure-caller regression");
}
console.log("ok");
