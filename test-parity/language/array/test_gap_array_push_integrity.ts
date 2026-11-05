// `Array.prototype.push` on a receiver whose integrity has been locked.
//
// §23.1.3.21 performs `Set(O, len, value, true)` and then
// `Set(O, "length", len+1, true)`. Both carry Throw=true, which is the
// mutator's OWN throw and does not depend on the caller's strictness — so
// every case below throws in this file whether node treats it as a module or
// as CommonJS.
//
// `preventExtensions` is the case that regressed: the dense append answered a
// non-extensible receiver with a bare `return arr`, which is correct for the
// INTERNAL CreateDataProperty-style append used to build fresh result arrays
// and wrong for user `push`. `seal` masked it, because sealing also marks the
// element descriptors and therefore routes the receiver down the exotic path,
// which threw for a different reason.
function push1(label: string, a: any): void {
  try {
    a.push(99);
    console.log(label, "no-throw len=" + a.length);
  } catch (e: any) {
    console.log(label, e.constructor.name + ": " + e.message);
  }
}

const ext: number[] = [1, 2];
Object.preventExtensions(ext);
push1("preventExtensions", ext);

const sealed: number[] = [1, 2];
Object.seal(sealed);
push1("seal", sealed);

const frozen: number[] = [1, 2];
Object.freeze(frozen);
push1("freeze", frozen);

// Zero-argument push only performs the `length` Set, which succeeds on a
// non-extensible receiver: no throw, length unchanged.
const zero: number[] = [1, 2];
Object.preventExtensions(zero);
zero.push();
console.log("zero-arg push len", zero.length);

// A non-writable `length` must still throw, and must keep throwing after the
// typed-feedback fast tier has been warmed by a hot loop — the re-check the
// guard performs per push is what this pins.
const warm: number[] = [];
for (let i = 0; i < 200; i++) {
  warm.push(i);
  warm.pop();
}
Object.defineProperty(warm, "length", { writable: false });
push1("warm-then-non-writable-length", warm);

// A plain array is unaffected by all of the above.
const plain: number[] = [1, 2];
plain.push(3);
console.log("plain push len", plain.length, "last", plain[plain.length - 1]);
