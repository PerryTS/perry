//! Typed-array construction and `length` on the web target.

use super::run_web_source;

// Every typed-array constructor, from a length, an array, an ArrayBuffer (with and without
// an offset and length) and another typed array, in a plain and an async body. Expected
// output is Node's.
const PROGRAM: &str = r#"
function show(label: string, t: any): string {
  const parts: string[] = [];
  for (let i = 0; i < t.length; i++) parts.push(String(t[i]));
  return label + " " + typeof t + " " + t.length + " " + t.byteLength + " " + t.byteOffset + " " + parts.join(",");
}
// BigInt elements are left out: a bigint value is not yet one on the web target.
function sized(label: string, t: any): string {
  return label + " " + typeof t + " " + t.length + " " + t.byteLength;
}
function run(): string[] {
  const ab = new ArrayBuffer(16);
  const w = new Int32Array(3);
  w[0] = 10; w[1] = 20.7; w[2] = -1;
  return [
    show("i8", new Int8Array([1, -2, 300])),
    show("u8", new Uint8Array([7, 8, 300])),
    show("u8c", new Uint8ClampedArray([7, -8, 300])),
    show("i16", new Int16Array([1, -2, 70000])),
    show("u16", new Uint16Array([1, -2])),
    show("i32", new Int32Array([1, 2, 1])),
    show("u32", new Uint32Array([1, -1])),
    show("f32", new Float32Array([1.5, 0.1])),
    show("f64", new Float64Array(2)),
    sized("bi64", new BigInt64Array(2)),
    sized("bu64", new BigUint64Array(1)),
    show("empty", new Int16Array()),
    show("ab", new Int32Array(ab, 4, 2)),
    show("u8ab", new Uint8Array(ab, 8)),
    show("ta", new Float64Array(new Int16Array([3, 4]))),
    show("write", w),
  ];
}
async function runAsync(): Promise<string[]> {
  const t = new Int32Array([5, 6]);
  return [show("async", t), show("async-u8", new Uint8Array([9])), show("async-f64", new Float64Array(1))];
}
console.log(run().join("\n"));
runAsync().then((lines) => console.log(lines.join("\n")));
"#;

#[test]
fn typed_arrays_are_the_hosts_and_have_a_length() {
    assert_eq!(
        run_web_source(PROGRAM),
        "i8 object 3 3 0 1,-2,44
u8 object 3 3 0 7,8,44
u8c object 3 3 0 7,0,255
i16 object 3 6 0 1,-2,4464
u16 object 2 4 0 1,65534
i32 object 3 12 0 1,2,1
u32 object 2 8 0 1,4294967295
f32 object 2 8 0 1.5,0.10000000149011612
f64 object 2 16 0 0,0
bi64 object 2 16
bu64 object 1 8
empty object 0 0 0 
ab object 2 8 4 0,0
u8ab object 8 8 8 0,0,0,0,0,0,0,0
ta object 2 16 0 3,4
write object 3 12 0 10,20,-1
async object 2 8 0 5,6
async-u8 object 1 1 0 9
async-f64 object 1 8 0 0"
    );
}

// `indexOf`, `lastIndexOf` and `includes` search a typed array as they search an array,
// fromIndex included: NaN is never found by index and `includes` finds it, `-0` is found
// as `0` (#15396). Expected output is Node's.
#[test]
fn typed_array_search_methods_match_node() {
    assert_eq!(
        run_web_source(
            r#"
const i = new Int32Array([1, 2, 3, 2, 1]);
const f = new Float64Array([NaN, 0.5, -0]);
const u = new Uint8Array([7, 8, 7]);
console.log([i.indexOf(2), i.indexOf(2, 2), i.lastIndexOf(2), i.lastIndexOf(2, 2), i.lastIndexOf(1, -2), i.lastIndexOf(9), i.includes(3), i.includes(3, 3)].join(","));
console.log([f.indexOf(NaN), f.includes(NaN), f.lastIndexOf(0), f.includes(0.5, -1), u.lastIndexOf(7), u.indexOf(8), u.includes(9)].join(","));
async function run(): Promise<string> {
  const a = new Int16Array([5, 6, 5]);
  return [a.lastIndexOf(5), a.lastIndexOf(5, 1), a.indexOf(6), a.includes(5, 3)].join(",");
}
run().then((s) => console.log(s));
"#
        ),
        "1,3,3,1,0,-1,true,false\n-1,true,2,false,2,1,false\n2,0,1,false"
    );
}

// The array methods a typed array shares run on it, each with the typed array's own
// semantics (`map` and `filter` make a typed array, `sort` is numeric). Expected output
// is Node's.
#[test]
fn typed_array_methods_match_node() {
    assert_eq!(
        run_web_source(
            r#"
const s = (v: any): string => (v && typeof v === "object" ? Object.prototype.toString.call(v).slice(8, -1) + "[" + Array.from(v as ArrayLike<number>).join(",") + "]" : String(v));
const mk = () => new Int16Array([3, 1, 2]);
const out: string[] = [];
const t = (name: string, f: () => any) => { try { out.push(name + "=" + s(f())); } catch (e) { out.push(name + "!" + (e as Error).name); } };
t("join", () => mk().join("-"));
t("join0", () => mk().join());
t("toString", () => mk().toString());
t("slice", () => mk().slice(1));
t("slice2", () => mk().slice(0, 2));
t("subarray", () => mk().subarray(1));
t("sort", () => mk().sort());
t("sortfn", () => mk().sort((a: number, b: number) => b - a));
t("map", () => mk().map((x: number) => x * 2));
t("filter", () => mk().filter((x: number) => x > 1));
t("fill", () => mk().fill(7));
t("fill2", () => mk().fill(7, 1, 2));
t("reverse", () => mk().reverse());
t("forEach", () => { let a = 0; mk().forEach((x: number) => { a += x; }); return a; });
t("reduce", () => mk().reduce((a: number, x: number) => a + x, 0));
t("reduceRight", () => mk().reduceRight((a: number, x: number) => a + "" + x, ""));
t("some", () => mk().some((x: number) => x > 2));
t("every", () => mk().every((x: number) => x > 0));
t("find", () => mk().find((x: number) => x < 3));
t("findIndex", () => mk().findIndex((x: number) => x === 2));
t("findLast", () => mk().findLast((x: number) => x > 1));
t("findLastIndex", () => mk().findLastIndex((x: number) => x > 1));
t("at", () => mk().at(-1));
t("set", () => { const a = mk(); a.set([9, 8], 1); return a; });
t("copyWithin", () => mk().copyWithin(0, 1));
t("entries", () => Array.from(mk().entries()).join("|"));
t("keys", () => Array.from(mk().keys()).join("|"));
t("values", () => Array.from(mk().values()).join("|"));
t("toReversed", () => mk().toReversed());
t("toSorted", () => mk().toSorted());
t("with", () => mk().with(0, 5));
t("from", () => Int16Array.from([1, 2]));
t("of", () => Float64Array.of(1, 2));
t("spread", () => [...mk()].join(","));
t("ArrayFrom", () => Array.from(mk()).join(","));
console.log(out.join("\n"));
"#
        ),
        "join=3-1-2\njoin0=3,1,2\ntoString=3,1,2\nslice=Int16Array[1,2]\nslice2=Int16Array[3,1]\nsubarray=Int16Array[1,2]\nsort=Int16Array[1,2,3]\nsortfn=Int16Array[3,2,1]\nmap=Int16Array[6,2,4]\nfilter=Int16Array[3,2]\nfill=Int16Array[7,7,7]\nfill2=Int16Array[3,7,2]\nreverse=Int16Array[2,1,3]\nforEach=6\nreduce=6\nreduceRight=213\nsome=true\nevery=true\nfind=1\nfindIndex=2\nfindLast=2\nfindLastIndex=2\nat=2\nset=Int16Array[3,9,8]\ncopyWithin=Int16Array[1,2,2]\nentries=0,3|1,1|2,2\nkeys=0|1|2\nvalues=3|1|2\ntoReversed=Int16Array[2,1,3]\ntoSorted=Int16Array[1,2,3]\nwith=Int16Array[5,1,2]\nfrom=Int16Array[1,2]\nof=Float64Array[1,2]\nspread=3,1,2\nArrayFrom=3,1,2"
    );
}

// `new DataView(buffer, offset?, length?)` is the host's DataView: its getters and
// setters, both byte orders, read and write the buffer the typed arrays share, in a
// plain and an async body. Expected output is Node's.
#[test]
fn data_view_is_the_hosts() {
    assert_eq!(
        run_web_source(
            r#"
const ab = new ArrayBuffer(16);
const dv = new DataView(ab);
dv.setInt16(0, -2);
dv.setUint32(2, 0xdeadbeef, true);
dv.setFloat64(8, 1.5);
dv.setInt8(6, -1);
console.log(typeof dv, dv.byteLength, dv.byteOffset, dv.buffer === ab, dv instanceof DataView);
console.log(dv.getInt16(0), dv.getUint16(0), dv.getUint32(2, true), dv.getUint32(2), dv.getFloat64(8), dv.getInt8(6), dv.getUint8(6));
const part = new DataView(ab, 4, 8);
console.log(part.byteLength, part.byteOffset, part.getUint8(0), new DataView(ab, 12).byteLength);
dv.setFloat32(12, 0.5, true);
console.log(dv.getFloat32(12, true), new Uint8Array(ab, 12, 4).join(","), Object.prototype.toString.call(dv));
const u8 = new Uint8Array([1, 2, 3, 4]);
console.log(new DataView(u8.buffer).getUint32(0), new DataView(u8.buffer, 1, 2).getUint16(0, true));
async function main(): Promise<void> {
  const d = new DataView(new ArrayBuffer(4));
  d.setUint16(0, 513);
  console.log("async", d.getUint8(0), d.getUint8(1), d.byteLength);
}
main();
"#
        ),
        "object 16 0 true true\n-2 65534 3735928559 4022250974 1.5 -1 255\n8 4 173 4\n0.5 0,0,0,63 [object DataView]\n16909060 770\nasync 2 1 4"
    );
}

// `Uint8Array.from` takes any array-like or iterable source, `slice` coerces its
// arguments, and `find` returns the element its predicate saw. Expected output is Node's.
#[test]
fn typed_array_from_slice_and_find_match_node() {
    assert_eq!(
        run_web_source(
            r#"
const t = new Int16Array([3, 1, 2]);
const a: any = [5, 6];
console.log(Uint8Array.from(new Uint8Array([7, 8])).join(","), Uint8Array.from(t).join(","), Uint8Array.from(new Set([4, 9])).join(","));
console.log(t.slice("1" as any).join(","), a.slice("1" as any).join(","), t.slice(0, "2" as any).join(","));
console.log(t.find((x: number) => { t[0] = 9; return true; }), a.find((x: number) => { a[0] = 0; return true; }));
async function run(): Promise<string> { return Uint8Array.from(new Int16Array([1, 2])).join(","); }
run().then((s) => console.log(s));
"#
        ),
        "7,8 3,1,2 4,9\n1,2 6 3,1\n3 5\n1,2"
    );
}
