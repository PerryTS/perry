//! Array methods inside an async body on the web target, which is emitted as JavaScript.

use super::{run_web_source, run_web_source_then};

// The callback methods of an array literal and of a local call their closures with the
// element, index and array, capture the body's locals, and reject when one throws.
// Expected output is Node's.
#[test]
fn async_body_array_callback_methods() {
    assert_eq!(
        run_web_source(
            r#"
async function f(n: number): Promise<string> {
  const xs = [3, 1, 2];
  let seen = "";
  xs.forEach((x, i, a) => { seen += x + ":" + i + ":" + a.length + ";"; });
  return [
    [1, 2].map(x => x * n).join(), [1, 2, 3].filter(x => x > 1).join(), xs.map((x, i) => x + i).join(),
    xs.filter(x => x > 1).length, xs.find(x => x < 3), xs.find(x => x > 9),
    xs.findIndex(x => x === 2), xs.findLast(x => x > 1), xs.findLastIndex(x => x > 9),
    xs.some(x => x > 2), xs.every(x => x > 2), xs.flatMap(x => [x, x]).join(""),
    xs.reduce((a, x) => a + x), xs.reduce((a, x) => a + x, "r"), xs.reduceRight((a, x) => a + x, "r"),
    xs.map((x) => x, 0).join(), seen,
  ].join(" ");
}
async function thrower(): Promise<string> {
  return [1].map(() => { throw new Error("boom"); }).join();
}
async function nullish(xs: any): Promise<string> {
  return xs.map((x: number) => x).join();
}
async function optional(xs: number[] | undefined): Promise<any> {
  return typeof xs?.map(x => x + 1);
}
async function awaited(): Promise<string> {
  const ps = (await Promise.resolve([1, 2])).map(x => Promise.resolve(x * 5));
  return (await Promise.all(ps)).join();
}
const show = (label: string, p: Promise<any>) => p.then((v) => console.log(label, v), (e) => console.log(label, "threw", e instanceof TypeError ? "TypeError" : e.message));
show("f", f(2));
show("thrower", thrower());
show("nullish", nullish(null));
show("optional", optional([1]));
show("absent", optional(undefined));
show("awaited", awaited());
"#
        ),
        "f 2,4 2,3 3,2,4 2 1  2 2 -1 true false 331122 6 r312 r213 3,1,2 3:0:3;1:1:3;2:2:3;\nthrower threw boom\nnullish threw TypeError\noptional object\nabsent undefined\nawaited 5,10"
    );
}

// The other array methods read and mutate the array in place, a module-level array and a
// local a closure also captures included, and evaluate their operands once. Expected output is Node's.
#[test]
fn async_body_array_methods() {
    assert_eq!(
        run_web_source(
            r#"
async function read(xs: number[]): Promise<string> {
  return [
    xs.indexOf(2), xs.indexOf(1, 2), xs.lastIndexOf(1), xs.includes(2), xs.includes(3, 3),
    xs.slice(1).join(), xs.slice(0, -1).join(), xs.at(-1), [[1], [2, [3]]].flat().length,
    xs.toReversed().join(), xs.toSorted().join(), xs.toSorted((a, b) => b - a).join(),
    xs.toSpliced(1, 2, 8, 9).join(), xs.toSpliced(2).join(), xs.with(0, 7).join(), xs.join(),
  ].join(" ");
}
const shared: number[] = [1, 2];
async function reach(s: Set<number>): Promise<string> {
  let t = 0;
  s.forEach(x => { t += x; });
  shared.push(...[3]);
  return [
    t, shared.map(x => x * 2).join(), shared.shift(), shared.indexOf(await Promise.resolve(3)),
    Array.prototype.map.call("ab", (c: any) => c + c).join(),
  ].join(" ");
}
async function mutate(): Promise<string> {
  let xs = [5, 10, 1];
  const len = () => xs.length;
  const out: any[] = [xs.pop(), xs.shift(), xs.unshift(4), xs.push(...[7, 8]), len()];
  let i = 0;
  out.push(xs.splice(i++, 1, 20, 21).join(), i, xs.splice(3).join(), xs.join());
  out.push(xs.sort((a, b) => a - b).join(), xs.sort().join(), xs.reverse().join(), xs.copyWithin(0, 1).join());
  out.push(xs.copyWithin(0, 2, 3).join(), [].pop(), [2, 1].sort((a, b) => a - b).join());
  return out.join(" ");
}
read([1, 2, 3, 1]).then((v) => console.log(v), (e) => console.log("threw " + e.message));
reach(new Set([1, 2])).then((v) => console.log(v), (e) => console.log("threw " + e.message));
mutate().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "1 3 3 true false 2,3,1 1,2,3 1 3 1,3,2,1 1,1,2,3 3,2,1,1 1,8,9,1 1,2 7,2,3,1 1,2,3,1\n1 5 2 4 4 4 1 7,8 20,21,10 10,20,21 10,20,21 21,20,10 20,10,10 10,10,10  1,2\n3 2,4,6 1 1 aa,bb"
    );
}

// A closure passed as data, not as the callback, keeps its identity. Expected output is
// Node's.
#[test]
fn async_body_array_methods_keep_closures_passed_as_data() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  const cb = () => 1;
  const xs: any[] = [cb];
  const out: any[] = [xs.includes(cb), xs.indexOf(cb), xs.lastIndexOf(cb), xs.with(0, cb)[0] === cb];
  xs.unshift(cb);
  out.push(xs[0] === cb, xs.toSpliced(0, 1, cb)[0] === cb, [1].reduce((a: any, x) => a, cb) === cb);
  xs.splice(0, 2, cb);
  xs.push(...[cb]);
  xs.push(cb, cb);
  out.push(xs[0] === cb, xs[1] === cb, xs[3] === cb, xs.map(x => x())[0]);
  return out.join(" ");
}
f().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "true 0 0 true true true true true true true 1"
    );
}

// A `thisArg` reaches a callback that reads `this`, which is undefined without one.
// Expected output is Node's.
#[test]
fn async_body_array_callbacks_take_this_arg() {
    assert_eq!(
        run_web_source(
            r#"
async function f(): Promise<string> {
  const o = { by: 10 };
  const add = function (this: any, x: number) { return this === undefined ? "none" : x + this.by; };
  return [[1, 2].map(add, o).join(), [1, 2].map(add).join(), [1, 2].map(x => x, o).join()].join(" ");
}
f().then((v) => console.log(v), (e) => console.log("threw " + e.message));
"#
        ),
        "11,12 none,none 1,2"
    );
}

// A program that replaces an array method still gets a callable callback, and a host
// method's call reads no same-named Array.prototype property.
#[test]
fn async_body_dispatch_uses_the_native_array_methods() {
    assert_eq!(
        run_web_source_then(
            r#"
async function f(xs: any, s: any): Promise<string> {
  return [xs.map((x: number) => x + 1).join(), s.union(s).size].join(" ");
}
const start = () => f([1, 2], new Set([7])).then((v) => console.log(v), (e) => console.log("threw " + e.message));
setTimeout(start, 20);
"#,
            r#"
const native = Array.prototype.map;
Array.prototype.map = function (cb) { return Reflect.apply(native, this, [(...a) => cb(...a)]); };
Object.defineProperty(Array.prototype, "union", { get() { throw new Error("read"); } });
"#
        ),
        "2,3 1"
    );
}
