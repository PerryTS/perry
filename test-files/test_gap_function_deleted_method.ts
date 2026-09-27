// A method deleted from a function object is gone: calling it throws a
// TypeError, and an own method that shadowed a Function.prototype method
// falls back to the inherited one once deleted.

function describe(label: string, run: () => unknown): void {
  try {
    const r = run();
    console.log(label, "returned", typeof r, JSON.stringify(r));
  } catch (e) {
    console.log(label, "threw", e instanceof TypeError ? "TypeError" : String(e));
  }
}

function plain(): number {
  return 1;
}
(plain as any).m = function (): number {
  return 2;
};
describe("own method before delete", () => (plain as any).m());
delete (plain as any).m;
describe("own method after delete", () => (plain as any).m());
console.log("has m after delete", "m" in plain, Object.prototype.hasOwnProperty.call(plain, "m"));

// Deleted after a read primed the call site.
const arrow = (x: number): number => x + 1;
(arrow as any).twice = (x: number): number => x * 2;
for (let i = 0; i < 3; i++) describe("primed call " + i, () => (arrow as any).twice(i));
delete (arrow as any).twice;
describe("primed call after delete", () => (arrow as any).twice(5));

// Several own keys; delete the middle one.
function multi(): void {}
(multi as any).a = () => "a";
(multi as any).b = () => "b";
(multi as any).c = () => "c";
delete (multi as any).b;
describe("multi a", () => (multi as any).a());
describe("multi b", () => (multi as any).b());
describe("multi c", () => (multi as any).c());

// An own `call` shadows Function.prototype.call; deleting it restores it.
function add(this: unknown, a: number, b: number): number {
  return a + b;
}
(add as any).call = () => "own call";
describe("own call", () => (add as any).call(null, 1, 2));
delete (add as any).call;
describe("inherited call after delete", () => add.call(null, 1, 2));

// Same for bind and apply.
(add as any).bind = () => "own bind";
(add as any).apply = () => "own apply";
delete (add as any).bind;
delete (add as any).apply;
describe("inherited bind after delete", () => add.bind(null, 3)(4));
describe("inherited apply after delete", () => add.apply(null, [5, 6]));

// A bound function's own method.
const bound = add.bind(null, 1);
(bound as any).q = () => "q";
delete (bound as any).q;
describe("bound own after delete", () => (bound as any).q());

// A method that was never there.
describe("never set", () => (plain as any).nope());

// A function whose prototype was replaced: its methods resolve there, and a
// name it lacks still throws.
function withProto(): void {}
Object.setPrototypeOf(withProto, {
  pm(this: unknown): string {
    return this === withProto ? "proto method, this ok" : "proto method, wrong this";
  },
});
describe("proto method", () => (withProto as any).pm());
describe("proto missing", () => (withProto as any).nope());

// Deleted, then re-added.
(plain as any).m = () => "again";
describe("re-added", () => (plain as any).m());

// A computed-key call of a deleted method.
const key = "dyn";
(plain as any)[key] = () => "dyn";
delete (plain as any)[key];
describe("computed after delete", () => (plain as any)[key]());
