// #10426: `Buffer.prototype`'s own (enumerable) property set had 36 bogus
// entries — including two bare string literals ("function"/"undefined" that
// had drifted in from nearby prose/JS-idiom comments, never real method
// names), plus DataView accessors, `Uint8Array.prototype`/TC39 base64-hex
// methods, and `Object.prototype` methods that all belong further up the
// prototype chain, not as OWN properties of Buffer.prototype — and was
// missing 16 of Node's own members: the 14 internal `<encoding>Slice`/
// `<encoding>Write` methods and the deprecated `offset`/`parent` accessors.
// Root cause: `BUFFER_PROTOTYPE_METHODS` in
// crates/perry-runtime/src/object/native_module/callable_exports.rs was
// generated from the SAME name table `buffer_dispatch::is_buffer_method_name`
// uses to decide whether a property READ on a Buffer *instance* should
// synthesize a bound-method closure (deliberately broad, for duck-typed
// inherited-method reads) — conflating that broad instance-read predicate
// with the narrow set of names that should be Buffer.prototype's own keys.
import { Buffer } from "node:buffer";

// The issue's own repro, verbatim (own/for-in counts + full sorted list).
const own = Object.getOwnPropertyNames(Buffer.prototype).sort();
const forin: string[] = [];
for (const k in Buffer.prototype) forin.push(k);
console.log("own", own.length, "for-in", forin.length);
console.log("has 'function':", own.includes("function"), "has 'undefined':", own.includes("undefined"));
console.log(own.join(" "));

// Descriptor shape for a representative sample of each own-property kind:
// a non-enumerable data method (constructor), ordinary enumerable data
// methods (old + new), and the two new accessor properties.
function describe(name: string) {
  const d = Object.getOwnPropertyDescriptor(Buffer.prototype, name);
  if (!d) {
    console.log(name, "MISSING");
    return;
  }
  console.log(
    name,
    JSON.stringify({
      writable: d.writable,
      enumerable: d.enumerable,
      configurable: d.configurable,
      hasGet: typeof d.get === "function",
      hasSet: typeof d.set === "function",
      isFn: typeof d.value === "function",
    }),
  );
}
for (const name of [
  "constructor",
  "write",
  "toString",
  "toLocaleString",
  "offset",
  "parent",
  "utf8Slice",
  "utf8Write",
  "hexSlice",
  "hexWrite",
  "base64Slice",
  "base64urlSlice",
  "asciiSlice",
  "latin1Slice",
  "ucs2Slice",
]) {
  describe(name);
}

// The bogus keys must be gone as OWN properties (still reachable as
// duck-typed INSTANCE reads through is_buffer_method_name, checked below —
// that's a distinct, intentionally-broader mechanism this fix doesn't touch).
console.log("own has 'at':", own.includes("at"));
console.log("own has 'set':", own.includes("set"));
console.log("own has 'hasOwnProperty':", own.includes("hasOwnProperty"));
console.log("own has 'toBase64':", own.includes("toBase64"));
console.log("own has 'getInt32':", own.includes("getInt32"));

// mysql2's own motivating idiom: no-op every function-typed key found via
// for-in on Buffer.prototype, on a zero-length Buffer.
const mock = Buffer.alloc(0);
let noopCount = 0;
for (const k of forin) {
  if (typeof (mock as any)[k] === "function") {
    (mock as any)[k] = () => {};
    noopCount++;
  }
}
console.log("noopCount:", noopCount);

// Functional round-trips for the 14 newly-added internal methods, on a
// fixed, deterministic byte source ("Hello").
const src = Buffer.from([0x48, 0x65, 0x6c, 0x6c, 0x6f]);
console.log("utf8Slice:", src.utf8Slice(0, 5));
console.log("asciiSlice:", src.asciiSlice(1, 4));
console.log("latin1Slice:", src.latin1Slice(0, 5));
console.log("hexSlice:", src.hexSlice(0, 5));
console.log("base64Slice:", src.base64Slice(0, 5));
console.log("base64urlSlice:", src.base64urlSlice(0, 5));

const u16 = Buffer.from("Hi", "utf16le");
console.log("ucs2Slice:", u16.ucs2Slice(0, 4));

// base64url-distinguishing bytes (produces '+'/'/' in base64, '-'/'_' in
// base64url) round-tripped through both Slice and Write.
const urlish = Buffer.from([0xfb, 0xff, 0xbf]);
console.log("base64Slice (urlish):", urlish.base64Slice(0, 3));
console.log("base64urlSlice (urlish):", urlish.base64urlSlice(0, 3));

const w1 = Buffer.alloc(8, 0);
console.log("utf8Write:", w1.utf8Write("Hi!", 1));
console.log("utf8Write result:", w1.toString("hex"));

const w2 = Buffer.alloc(8, 0);
console.log("hexWrite:", w2.hexWrite("48656c6c6f", 0));
console.log("hexWrite result:", w2.toString("utf8", 0, 5));

const w3 = Buffer.alloc(8, 0);
console.log("base64Write:", w3.base64Write("SGVsbG8=", 0));
console.log("base64Write result:", w3.toString("utf8", 0, 5));

const w4 = Buffer.alloc(8, 0);
console.log("base64urlWrite:", w4.base64urlWrite("SGVsbG8", 0));
console.log("base64urlWrite result:", w4.toString("utf8", 0, 5));

const w5 = Buffer.alloc(8, 0);
console.log("asciiWrite:", w5.asciiWrite("Hi", 2));
console.log("asciiWrite result:", w5.toString("ascii", 2, 4));

const w6 = Buffer.alloc(8, 0);
console.log("latin1Write:", w6.latin1Write("Hi", 0));
console.log("latin1Write result:", w6.toString("latin1", 0, 2));

const w7 = Buffer.alloc(8, 0);
console.log("ucs2Write:", w7.ucs2Write("Hi", 0));
console.log("ucs2Write result:", w7.toString("utf16le", 0, 4));

// Cross-check against the existing generic `toString(encoding, start, end)`
// / `write(string, offset, length, encoding)` paths these delegate to — must
// agree exactly.
console.log("utf8Slice === toString(utf8):", src.utf8Slice(0, 5) === src.toString("utf8", 0, 5));
console.log("hexSlice === toString(hex):", src.hexSlice(0, 5) === src.toString("hex", 0, 5));

// `offset`/`parent` on a real instance (already correctly handled at the
// instance level before this fix — kept as a regression control) and a
// subarray with a non-zero byteOffset.
const backing = Buffer.alloc(16);
const view = backing.subarray(4, 10);
console.log("view.offset:", (view as any).offset, "view.byteOffset:", view.byteOffset);
console.log("view.parent === view.buffer:", (view as any).parent === view.buffer);

// The new accessor's `get`, invoked directly off Buffer.prototype (the
// reflection path the accessor descriptor itself exists for).
const offsetGetter = Object.getOwnPropertyDescriptor(Buffer.prototype, "offset")!.get!;
console.log("offset getter via .call(view):", offsetGetter.call(view));
console.log("offset getter via .call({}) (non-buffer this):", offsetGetter.call({}));

// Regression: instance-level duck-typed reads for names removed from the
// OWN-property list must still work (inherited, not own).
const b = Buffer.alloc(4);
console.log("typeof b.hasOwnProperty:", typeof b.hasOwnProperty);
console.log("b.hasOwnProperty('x'):", b.hasOwnProperty("x"));
console.log("typeof b.at:", typeof b.at);
console.log("b.at(0):", b.at(0));
console.log("Buffer.prototype.hasOwnProperty('at'):", Buffer.prototype.hasOwnProperty("at"));
console.log("Buffer.prototype.hasOwnProperty('hasOwnProperty'):", Buffer.prototype.hasOwnProperty("hasOwnProperty"));
