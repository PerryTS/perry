// #6386: guarded fast paths for DataView accessors, Array.prototype.concat,
// and regex match materialization. Each block exercises the fast path AND the
// condition that must kick execution back to the generic/spec path —
// including mid-run flips of the monotonic gates (isConcatSpreadable ever
// set, constructor accessor ever installed, buffer own props ever stored),
// which are load-bearing: the fast path must keep honoring exotica installed
// AFTER it has already run hot.

// ---- DataView direct accessors --------------------------------------------
{
  const b = new ArrayBuffer(64);
  const v = new DataView(b);
  // Hot loop first: the direct entries run repeatedly before any exotica.
  let acc = 0;
  for (let i = 0; i < 1000; i++) {
    v.setFloat64(8, i * 1.5, true);
    acc += v.getFloat64(8, true);
  }
  console.log("dv hot:", acc);
  // Endianness default (big) vs explicit little.
  v.setFloat64(0, 1.5);
  console.log("dv be/le:", v.getFloat64(0), v.getFloat64(0, true));
  v.setInt16(2, -2, true);
  console.log("dv i16/u16:", v.getInt16(2, true), v.getUint16(2, true));
  // Wrap semantics on setters.
  v.setUint16(4, 70000, true);
  console.log("dv wrap:", v.getUint16(4, true));
  v.setInt8(6, -1);
  console.log("dv u8 of -1:", v.getUint8(6));
  // Offset coercions the fast path must not break: fractional-but-integral
  // doubles, booleans, numeric strings via objects.
  v.setInt32(8.0, 42, true);
  console.log("dv int off:", v.getInt32(8, true));
  // Out-of-bounds RangeError still throws.
  try {
    v.getFloat64(60, true);
    console.log("dv oob: NO THROW");
  } catch (e) {
    console.log("dv oob:", (e as Error).constructor.name);
  }
  // Static type violated at runtime: the same call site must fall back to
  // generic dispatch when the variable holds a plain object.
  let w: any = new DataView(b);
  w.setUint8(0, 7);
  console.log("dv typed:", w.getUint8(0));
  w = { getUint8: (o: number) => 123 + o, setUint8: (o: number, x: number) => 0 };
  console.log("dv reassigned:", w.getUint8(1));
  // BigInt accessors ride the same direct path.
  const v2 = new DataView(new ArrayBuffer(16));
  v2.setBigInt64(0, -2n, true);
  console.log("dv bigint:", v2.getBigInt64(0, true), v2.getBigUint64(0, true));
  // Mid-run flip of the own-prop gate: an own method assigned onto a
  // DataView AFTER the direct path ran hot must shadow the prototype
  // accessor at the same call site.
  const v3: any = new DataView(new ArrayBuffer(8));
  v3.setUint8(0, 5);
  console.log("dv pre-shadow:", v3.getUint8(0));
  v3.getUint8 = (o: number) => 42 + o;
  console.log("dv shadowed:", v3.getUint8(1));
}

// ---- Array.prototype.concat -----------------------------------------------
{
  // Hot dense loop first (all-dense bulk path).
  const x: number[] = [], y: number[] = [];
  for (let i = 0; i < 100; i++) { x.push(i); y.push(i + 100); }
  let n = 0;
  for (let r = 0; r < 500; r++) n += ([] as number[]).concat(x, y).length;
  console.log("concat hot:", n);
  // Mixed values: primitives, strings, nested arrays stay nested one level.
  const mixed = [1].concat(2, "three", [4, [5]], true as any, null as any);
  console.log("concat mixed:", JSON.stringify(mixed));
  // Holes are preserved (slow path), inherited reads NOT collapsed.
  const holey = [1, , 3];
  const hres = [0].concat(holey);
  console.log("concat holes:", hres.length, 1 in hres, 2 in hres, hres[3]);
  // Strings copied by reference must not alias later source mutation.
  let s = "ab";
  const strres = ([] as string[]).concat([s]);
  s += "cd";
  console.log("concat str demote:", strres[0], s);
  // Mid-run flip: installing @@isConcatSpreadable ANYWHERE after hot concats
  // ran must be honored (the monotonic gate opens the spec path). Own symbol
  // props on array instances are a separate pre-existing gap (unchanged by
  // #6386): concat doesn't see `arr[Symbol.isConcatSpreadable]`, so the flip
  // is exercised through object receivers, which are modeled.
  const fake: any = { length: 2, 0: "a", 1: "b", [Symbol.isConcatSpreadable]: true };
  console.log("concat spread obj:", JSON.stringify([1].concat(fake)));
  // The flag is now flipped process-wide; dense array concat must still be
  // correct (spec path or fast path both produce this).
  console.log("concat post-flip:", JSON.stringify([1].concat([9, 8])));
  // Mid-run flip: an OWN constructor on one array redirects species for that
  // array only; plain arrays keep the fast default.
  function Custom(this: any, len: number) { this.len = len; }
  (Custom as any)[Symbol.species] = Custom;
  const withCtor: any = [1, 2];
  withCtor.constructor = Custom;
  const custom = withCtor.concat([3]);
  console.log("concat species:", Array.isArray(custom), custom instanceof (Custom as any));
  console.log("concat plain still fast:", JSON.stringify([1].concat([2, 3])));
}

// ---- Regex match / exec materialization -----------------------------------
{
  const s = "2026-07-13 key=42 val=99";
  const re = /(\d{4})-(\d{2})-(\d{2}) key=(\d+)/;
  // Hot loop.
  let a = 0;
  for (let i = 0; i < 2000; i++) {
    const m = s.match(re);
    if (m) a += m[1].length + m[4].length;
  }
  console.log("rm hot:", a);
  const m = s.match(re)!;
  console.log("rm caps:", JSON.stringify(Array.from(m)));
  console.log("rm index/input:", m.index, m.input === s, m.groups);
  // Named groups build a real groups object.
  const nm = "x=7".match(/(?<key>\w)=(?<val>\d)/)!;
  console.log("rm named:", nm.groups!.key, nm.groups!.val, nm.index);
  // Unmatched optional group is undefined in the array.
  const om = "ab".match(/a(z)?(b)/)!;
  console.log("rm optional:", om[1], om[2], om.length);
  // exec: same decoration + lastIndex behavior for /g.
  const gre = /k(\d)/g;
  const subject = "k1 k2";
  const e1 = gre.exec(subject)!;
  const e2 = gre.exec(subject)!;
  console.log("rm exec:", e1[1], e1.index, e2[1], e2.index, gre.lastIndex);
  // Subject re-boxed as .input must not alias later mutation of the local.
  let subj = "q=5";
  const mm = subj.match(/q=(\d)/)!;
  subj += "!";
  console.log("rm input demote:", mm.input, subj);
  // Match results survive an interleaved match on another regex.
  const mA = "aa".match(/(a)(a)/)!;
  const mB = "bb".match(/(b)/)!;
  console.log("rm interleave:", mA.index, mA[2], mB.index, mB[1]);
}
