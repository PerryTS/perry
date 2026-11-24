//! `Array.from` on the web target, in a synchronous body (WASM) and in an async body
//! (emitted as JavaScript).

use super::run_web_source;

// An async body builds `Array.from` of a Map's keys and values, a Map, a Set, an array,
// an array-like, a string and an awaited value, and applies a map function with its
// index and `thisArg`, as a synchronous one does. Expected output is Node's.
#[test]
fn array_from_matches_node_in_sync_and_async_bodies() {
    assert_eq!(
        run_web_source(
            r#"
const tenX = (v: number) => v * 10;
const plusIndex = (v: number, i: number) => v + i;
const square = (_: unknown, i: number) => i * i;
function dbl(v: number) { return v * 2; }
const plusK = function (this: any, v: number) { return v + this.k; };
function sync(): string {
  const m = new Map<string, number>([["a", 1], ["b", 2]]);
  return [
    Array.from(m.keys()).join(","),
    Array.from(m.values(), (v) => v * 10).join(","),
    Array.from({ length: 3 }, (_, i) => i * i).join(","),
    Array.from([1, 2], function (this: any, v: number) { return v + this.k; }, { k: 100 }).join(","),
  ].join(" ");
}
async function inAsync(): Promise<string> {
  const m = new Map<string, number>([["a", 1], ["b", 2]]);
  const s = new Set<number>([1, 2]);
  const o: any = { length: 2, 0: "x", 1: "y" };
  const a = await Promise.resolve([3, 4]);
  return [
    Array.from(m.keys()).join(","),
    Array.from(m.values()).join(","),
    Array.from(m).length,
    Array.from(s).join(","),
    Array.from([1, 2]).join(","),
    Array.from(o).join(","),
    Array.from("hé").join("|"),
    Array.from(a).join(","),
    Array.from(m.values(), tenX).join(","),
    Array.from([5, 6], plusIndex).join(","),
    Array.from({ length: 3 }, square).join(","),
    Array.from([1, 2], plusK, { k: 100 }).join(","),
    Array.from([1, 2], dbl).join(","),
    Array.from([1, 2], (v) => v + 1).join(","),
  ].join(" ");
}
console.log(sync());
inAsync().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "a,b 10,20 0,1,4 101,102\na,b 1,2 2 1,2 1,2 x,y h|é 3,4 10,20 5,7 0,1,4 101,102 2,4 2,3"
    );
}

// A map function that throws stops the iteration and closes the iterator before the
// enclosing `catch` runs; an undefined one maps nothing. Expected output is Node's.
#[test]
fn array_from_stops_at_a_throwing_map_function() {
    assert_eq!(
        run_web_source(
            r#"
let calls = 0;
function bad(v: number) { calls++; if (v === 2) throw new Error("at " + v); return v; }
function* gen() { try { yield 1; yield 2; yield 3; } finally { console.log("closed"); } }
try { Array.from(gen(), bad); console.log("no throw"); } catch (e: any) { console.log(e.message, calls); }
const none: any = undefined;
console.log(Array.from([1, 2], none).join(","));
async function inAsync() {
  calls = 0;
  try { Array.from([1, 2, 3], bad); console.log("no throw"); } catch (e: any) { console.log(e.message, calls); }
  console.log(Array.from([1, 2], none).join(","));
}
inAsync();
"#
        ),
        "closed\nat 2 2\n1,2\nat 2 2\n1,2"
    );
}

// A mapFn that is not callable is a TypeError, even for an empty source; an undefined
// one maps nothing. Expected output is Node's.
#[test]
fn array_from_with_a_non_callable_map_fn_throws_type_error() {
    assert_eq!(
        run_web_source(
            r#"
const bad: any[] = [42, {}, "x", null];
function sync(): string {
  const out: string[] = [];
  for (const f of bad) {
    try {
      out.push(JSON.stringify(Array.from([1, 2], f)));
    } catch (e: any) {
      out.push((e instanceof TypeError) + ":" + e.message);
    }
  }
  try {
    out.push(JSON.stringify(Array.from([], bad[0])));
  } catch (e: any) {
    out.push((e instanceof TypeError) + ":" + e.message);
  }
  out.push(JSON.stringify(Array.from([1, 2], undefined)));
  return out.join(" | ");
}
async function inAsync(): Promise<string> {
  const src = await Promise.resolve([1, 2]);
  try {
    return JSON.stringify(Array.from(src, bad[0]));
  } catch (e: any) {
    return (e instanceof TypeError) + ":" + e.message;
  }
}
console.log(sync());
inAsync().then((v) => console.log(v), (e) => console.log("rejected " + e.message));
"#
        ),
        "true:number 42 is not a function | true:object is not a function | true:string \"x\" is not a function | true:object null is not a function | true:number 42 is not a function | [1,2]\ntrue:number 42 is not a function"
    );
}
