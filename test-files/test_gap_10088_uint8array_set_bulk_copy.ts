// #10088 — Uint8Array/Buffer.prototype.set(source, offset) went through a
// per-byte view-table lookup (and, for Buffer/TypedArray sources, an
// intermediate Vec<u8>) instead of a bulk copy. The fix resolves the view
// indirection once per call for Buffer and same-element-width (1-byte)
// TypedArray sources and copies the raw span directly. This exercises every
// source arm plus the overlap / view-coherency guarantees the bulk path must
// preserve.

// `instanceof`, not `e.constructor.name`: a pre-existing, unrelated gap in
// the ERR_OUT_OF_RANGE error path (#buffer/numeric.rs's `throw_range_error_
// code`) leaves `.constructor` undefined on that particular RangeError, so
// asserting the class this way stays independent of that separate bug.
function r(fn: () => unknown): string {
    try {
        return "ok:" + String(fn());
    } catch (e: any) {
        if (e instanceof RangeError) return "throw:RangeError";
        if (e instanceof TypeError) return "throw:TypeError";
        return "throw:" + (e && e.name ? e.name : String(e));
    }
}

function show(a: Uint8Array): string {
    return Array.from(a).join(",");
}

// ---- Buffer-to-Buffer (the bulk-copy fast path) ----
{
    const dest = Buffer.alloc(6);
    const src = Buffer.from([10, 20, 30]);
    dest.set(src, 2);
    console.log("buffer-to-buffer:", show(dest));
}

// ---- Uint8Array-to-Uint8Array ----
{
    const dest = new Uint8Array(5);
    const src = new Uint8Array([1, 2, 3]);
    dest.set(src, 1);
    console.log("u8-to-u8:", show(dest));
}

// ---- Int8Array source: negative values must wrap to the same byte a
// two's-complement reinterpretation gives (-1 -> 255, -128 -> 128). ----
{
    const dest = new Uint8Array(4);
    const src = new Int8Array([-1, -128, 127, 0]);
    dest.set(src);
    console.log("int8-source:", show(dest));
}

// ---- Uint8ClampedArray source: already-clamped bytes copy as-is. ----
{
    const dest = new Uint8Array(3);
    const src = new Uint8ClampedArray([0, 128, 255]);
    dest.set(src);
    console.log("uint8clamped-source:", show(dest));
}

// ---- Multi-byte TypedArray source: still needs per-element ToNumber-style
// coercion (NOT the bulk path) — Float64Array carrying fractional/huge/NaN
// values. ----
{
    const dest = new Uint8Array(4);
    const src = new Float64Array([300, -1, NaN, 3.9]);
    dest.set(src);
    console.log("float64-source:", show(dest));
}

// ---- Plain Array source needing ToUint8 wrapping. ----
{
    const dest = new Uint8Array(5);
    dest.set([-1, 256, 300.9, NaN, Infinity]);
    console.log("array-source:", show(dest));
}

// ---- Array-like Object source with index-named properties. ----
{
    const dest = new Uint8Array(3);
    dest.set({ length: 3, 0: 7, 1: 8, 2: 9 } as any);
    console.log("object-source:", show(dest));
}

// ---- Zero-length source: no-op, destination untouched. ----
{
    const dest = new Uint8Array([1, 2, 3]);
    dest.set(new Uint8Array(0), 1);
    console.log("zero-length-source:", show(dest));
}

// ---- Out-of-range offset: RangeError, target untouched. ----
{
    const dest = new Uint8Array(3);
    console.log("out-of-range-offset:", r(() => dest.set(new Uint8Array(2), 5)));
    console.log("out-of-range-offset target:", show(dest));
}

// ---- BigInt-kind source mixed with a Number-kind target must still throw
// (unaffected by the bulk path — BigInt64/BigUint64 never take it). ----
{
    const dest = new Uint8Array(3);
    console.log("bigint-mix:", r(() => dest.set(new BigInt64Array([1n, 2n]) as any)));
}

// ---- Overlap: forward shift (`buf.set(buf.subarray(2))`, dest offset < the
// subarray's own start — reading must observe the ORIGINAL bytes throughout,
// like memmove). ----
{
    const buf = new Uint8Array([1, 2, 3, 4, 5, 6]);
    buf.set(buf.subarray(2), 0);
    console.log("overlap-forward:", show(buf));
}

// ---- Overlap: backward shift (dest offset > source's own start). ----
{
    const buf = new Uint8Array([1, 2, 3, 4, 5, 6]);
    buf.set(buf.subarray(0, 4), 2);
    console.log("overlap-backward:", show(buf));
}

// ---- Overlap: source fully nested inside the destination's own window. ----
{
    const buf = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]);
    buf.set(buf.subarray(2, 5), 3);
    console.log("overlap-nested:", show(buf));
}

// ---- #1205 view coherency: a direct write to the BACKING buffer (the
// codegen fast path for a statically-typed Buffer.alloc local) must still be
// visible to a `set()` that reads through a registered view of it. ----
{
    const backing = Buffer.alloc(4);
    backing[0] = 1;
    backing[1] = 2;
    backing[2] = 3;
    backing[3] = 4;
    const view = backing.subarray(1, 3);
    backing[2] = 99; // direct write to the backing AFTER the view was created
    const dest = Buffer.alloc(2);
    dest.set(view);
    console.log("view-coherency:", show(dest));
}
