//! `Array.from` on the web target, in a synchronous body (WASM) and in an async body
//! (emitted as JavaScript).

use super::run_web_source;

// An async body builds `Array.from` of a Map's keys and values, a Map, a Set, an array,
// an array-like, a string and an awaited value, and applies a map function with its
// index and `thisArg`, as a synchronous one does. The map functions are created outside
// the async body, which has no closure arm of its own (#15128). Expected output is Node's.
#[test]
fn array_from_matches_node_in_sync_and_async_bodies() {
    assert_eq!(
        run_web_source(
            r#"
const tenX = (v: number) => v * 10;
const plusIndex = (v: number, i: number) => v + i;
const square = (_: unknown, i: number) => i * i;
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
  ].join(" ");
}
console.log(sync());
inAsync().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "a,b 10,20 0,1,4 101,102\na,b 1,2 2 1,2 1,2 x,y h|é 3,4 10,20 5,7 0,1,4 101,102"
    );
}
