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
