// DPA-F1 #1: compiled state that agents share (module-state globals,
// constants, birth images) holds only STATIC LayoutWords. perry/thread
// workers never run module init and allocate dynamic rows from their own
// chunks, so a class birth word stored in a shared global by the spawner
// would name an unpublished row in the worker. Every compiler-known class
// layout carries a static ordinal; workers birth ConstFn, wide, literal and
// inherited classes against the same words the main agent uses.
// perry-only (`perry/thread` has no Node equivalent).
import { parallelMap, spawn } from "perry/thread";

function helper(): number { return 1; }
class Fn { cb: () => number; n: number; constructor() { this.cb = helper; this.n = 2; } }
class Field { g = function (): number { return 3; }; z = 4; }
class Arrow { m = (): number => 5; }
class Wide { constructor() { (this as any)["k" + 1] = 6; (this as any).late = 7; } }
class Base { b = 8; }
class Der extends Base { d = 9; }
const lit = { m: helper, k: 10 };

function work(i: number): number {
  let sum = 0;
  for (let j = 0; j < 50; j++) {
    const f = new Fn();
    const g = new Field();
    const a = new Arrow();
    const w = new Wide() as any;
    const d = new Der();
    const o = { m: helper, k: i };
    sum += f.cb() + f.n + g.g() + g.z + a.m() + w.k1 + w.late + d.b + d.d + o.m() + o.k + lit.k;
  }
  return sum;
}

const expected = [0, 1, 2, 3, 4, 5, 6, 7].map(work);
console.log("main", expected.join(","));
const results = parallelMap([0, 1, 2, 3, 4, 5, 6, 7], (i: number) => work(i));
console.log("parallelMap", results.join(","), results.join(",") === expected.join(","));
spawn(() => work(3)).then((v: number) => console.log("spawn", v, v === expected[3]));
