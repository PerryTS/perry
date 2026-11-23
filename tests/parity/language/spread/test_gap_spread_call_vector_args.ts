// The vector form of a spread call (the argument array the drain built, read into a fixed-arity
// body's parameters): slots below the array's dense prefix are loaded directly, any other slot
// is the runtime's element read. Holes, short and long lists, iterators and array-likes, into
// class methods selected by class id, module functions and closures. Compared against Node.
const show = (...xs: any[]) => JSON.stringify(xs.map((x) => (x === undefined ? "u" : x)));
class A { tag = "A"; m(a: any, b: any, c: any, d: any) { return this.tag + show(a, b, c, d); } }
class B { tag = "B"; m(a: any, b: any) { return this.tag + show(a, b) + arguments.length; } }
class C { tag = "C"; m(...xs: any[]) { return this.tag + show(...xs); } }
function modFn(a: any, b: any, c: any) { return "f" + show(a, b, c); }
const closure = (a: any, b: any) => "k" + show(a, b);

function* gen(n: number) { for (let i = 0; i < n; i++) yield { i }; }
const sources: any[] = [
  [], [1], [1, 2, 3, 4, 5, 6], [1, , 3], [, , "x"], new Array(3), "hey", new Set(["s", { o: 1 }]),
  new Map([[1, 2]]), gen(0), gen(2), gen(5), [{ p: 1 }, [2], "three"],
];
const recvs: any[] = [new A(), new B(), new C()];
for (const src of sources) {
  const row: string[] = [];
  for (const r of recvs) {
    const copy = typeof src === "object" && src !== null && !Array.isArray(src) && typeof src.next === "function" ? null : src;
    row.push(copy === null ? "gen" : r.m(...copy));
    row.push(copy === null ? "gen" : r.m(0, ...copy, 9));
  }
  if (Array.isArray(src) || typeof src === "string") row.push(modFn(...src), closure(...src));
  console.log(row.join(" "));
}
// Generators are consumed once: one spread each.
console.log(new A().m(...gen(3)), new B().m(...gen(1), "t"), modFn(...gen(4)));
// Many calls with heap values, so the argument vectors are collected and reused.
let acc = 0;
for (let i = 0; i < 20000; i++) {
  const r = recvs[i % 3];
  const out = r.m(...[{ v: i }, "s" + (i & 7), [i]]);
  acc += out.length;
}
console.log("acc", acc);
