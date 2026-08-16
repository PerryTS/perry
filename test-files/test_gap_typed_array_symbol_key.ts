// A Symbol key on a typed-array receiver must do OrdinarySet (ECMA-262
// §10.4.5.5: a key that is not a CanonicalNumericIndexString is not an index).
// Perry's computed-store path let the numeric-index arm claim the write: a
// Symbol is a NaN-boxed pointer, which as an f64 is a NaN, so it was
// classified a "canonical-invalid index" and dropped in silence.

const s: any = Symbol("x");

// Every receiver kind must behave the same. Only the typed array was broken.
const o: any = {};
o[s] = 5;
console.log("obj:", o[s], Object.getOwnPropertySymbols(o).length);

const arr: any = [1, 2];
arr[s] = 5;
console.log("arr:", arr[s], Object.getOwnPropertySymbols(arr).length);

const buf: any = Buffer.alloc(2);
buf[s] = 5;
console.log("buf:", buf[s], Object.getOwnPropertySymbols(buf).length);

const u8: any = new Uint8Array([1, 2]);
u8[s] = 5;
console.log("u8:", u8[s], Object.getOwnPropertySymbols(u8).length);

// The element store through the same helper must be undisturbed.
u8[1] = 9;
console.log("elements:", u8[0], u8[1]);

// The user-visible consequence: a typed array could not opt in to
// @@isConcatSpreadable by assignment, though defineProperty worked.
const a: any = new Uint8Array([9, 10]);
a[Symbol.isConcatSpreadable] = true;
console.log("optin readback:", a[Symbol.isConcatSpreadable]);
console.log("optin concat:", JSON.stringify([1].concat(a)));

const b: any = new Uint8Array([9, 10]);
Object.defineProperty(b, Symbol.isConcatSpreadable, { value: true, configurable: true });
console.log("defineProperty concat:", JSON.stringify([1].concat(b)));

// Default (no opt-in): a typed array is NOT concat-spreadable.
console.log("default concat:", JSON.stringify([1, 2].concat(new Uint8Array([3, 4]))));
