// #7769: the class parent chain moved from a process-global `RwLock<HashMap>`
// to a dense atomic mirror, and virtual dispatch grew a THREAD-LOCAL cache of
// the dispatch tower's resolution.
//
// `perry/thread` runs real OS threads with independent arenas, so both need to
// stay correct off the main thread: the parent mirror is shared and must be
// visible to a worker (an unregistered-looking chain would break `instanceof`
// and `super()`), while each worker starts with an EMPTY dispatch cache and
// must populate it from its own tower run rather than inheriting one.
//
// perry-only (`perry/thread` has no Node equivalent), so this is an
// `test_issue_*` behavioural test, not a byte-for-byte gap test.
import { parallelMap, spawn } from "perry/thread";

class Shape {
  size: number;
  constructor(size: number) {
    this.size = size;
  }
  area(): number {
    return this.size;
  }
  name(): string {
    return "shape";
  }
}
class Box extends Shape {
  area(): number {
    return this.size * this.size;
  }
  name(): string {
    return "box";
  }
}
// The fieldless / indirect shapes CLAUDE.md flags as weak.
class Marker extends Shape {}
class Cube extends Box {
  area(): number {
    return this.size * this.size * this.size;
  }
}

function describe(n: number): string {
  const shapes: Shape[] = [
    new Shape(n),
    new Box(n),
    new Marker(n),
    new Cube(n),
  ];
  let out = "";
  for (let i = 0; i < shapes.length; i++) {
    const s = shapes[i];
    out =
      out +
      s.name() +
      ":" +
      s.area() +
      ":" +
      (s instanceof Box ? "B" : "-") +
      (s instanceof Shape ? "S" : "-") +
      " ";
  }
  return out.trim();
}

// Main thread first, so the dispatch cache and the parent mirror are already
// warm when the workers start — a worker that wrongly READ the main thread's
// cache, or that failed to see the parent edges, diverges from this string.
const expected = describe(3);
console.log("main:", expected);

// parallelMap: many workers, each building and dispatching its own instances.
const mapped = parallelMap([3, 3, 3, 3, 3, 3, 3, 3], (n: number): string =>
  describe(n),
);
let allMatch = true;
for (let i = 0; i < mapped.length; i++) {
  if (mapped[i] !== expected) allMatch = false;
}
console.log("parallelMap count:", mapped.length, "allMatch:", allMatch);

// spawn: a single background OS thread.
const spawned = await spawn((): string => describe(3));
console.log("spawn:", spawned, "match:", spawned === expected);

// The main thread must still be correct after the workers have run.
console.log("main again:", describe(3) === expected);
