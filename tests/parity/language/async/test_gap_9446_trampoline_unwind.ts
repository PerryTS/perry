// #9446: the runtime's all-f64 call trampoline (`abi_trampoline::call_all_f64`)
// is what every dynamic vtable dispatch and every dynamic `new` on a class
// value goes through when the callee's arity is only known at runtime. With
// more than eight f64 arguments (`this` counts) it lowers the stack pointer by
// a runtime amount to spill the rest — and, as an inline asm block inside an
// ordinary Rust function, it did so behind a frame description that still said
// the stack pointer had not moved. Every unwinder stepping through the
// trampoline while the callee ran then read the trampoline's return address
// from a spilled argument or a saved register:
//
//   * a `throw` inside the callee never found the `catch` above the trampoline
//     (the exception transport is the system unwinder on x86-64), so a caught
//     exception was reported as uncaught;
//   * a collection inside the callee never saw the frames ABOVE the
//     trampoline, so their roots were dropped: a young object the caller holds
//     across the call is not copied, and the caller reads a recycled cell.
//
// Neither showed on aarch64, where LLVM keeps a frame pointer for the
// trampoline function and the description is frame-pointer-relative. On
// x86-64 Linux the second shape is `PERRY_GC_SCHEDULE_SEED=1
// PERRY_GC_SCHEDULE_RATE=1` crashing the Claude Code bundle at safepoint 4266.
//
// Every dispatch below is dynamic ON PURPOSE — a computed method name, or a
// class value the `new` site cannot see through — so the call reaches the
// runtime vtable path and the trampoline rather than an inlined class-id
// tower. Nine declared parameters plus `this` puts two arguments on the stack.
//
// Expected output:
// throw-through-method: from wide 36
// throw-through-ctor: ctor 45
// after-churn: 7/seven wide:36
// control: other:36

class Wide {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
  probe(
    a: number,
    b: number,
    c: number,
    d: number,
    e: number,
    f: number,
    g: number,
    h: number,
    mode: string,
  ): string {
    const sum = a + b + c + d + e + f + g + h;
    if (mode === "throw") {
      throw new Error("from " + this.tag + " " + sum);
    }
    if (mode === "churn") {
      churn(600000);
    }
    return this.tag + ":" + sum;
  }
}

// A second implementor keeps `probe` a genuine dispatch, not a single callee.
class Other {
  tag: string = "other";
  probe(
    a: number,
    b: number,
    c: number,
    d: number,
    e: number,
    f: number,
    g: number,
    h: number,
    _mode: string,
  ): string {
    return this.tag + ":" + (a + b + c + d + e + f + g + h);
  }
}

// `any`, so the receiver's class is not statically known.
function make(k: number): any {
  return k % 2 === 0 ? new Wide("wide") : new Other();
}

// A computed member name: the call site cannot pick an implementor, so the
// method is resolved by name at runtime and invoked through the vtable.
function methodName(): string {
  return "pro" + "be";
}

// A class EXPRESSION handed out as a value: `new` on it replays the constructor
// through the runtime constructor table — the same trampoline. Its arity is
// nine user params plus the synthesized capture params.
function makeClass(label: string): any {
  return class {
    total: number;
    constructor(
      a: number,
      b: number,
      c: number,
      d: number,
      e: number,
      f: number,
      g: number,
      h: number,
      i: number,
    ) {
      this.total = a + b + c + d + e + f + g + h + i;
      throw new Error(label + " " + this.total);
    }
  };
}

// Allocates past the 16 MiB nursery cap with cells that ESCAPE (they are
// pushed into an array), so a copying minor runs at a loop poll INSIDE the
// dynamically dispatched callee. A scalar-replaced literal would allocate
// nothing and the probe could not fail.
function churn(n: number): void {
  let keep: any[] = [];
  for (let i = 0; i < n; i++) {
    const cell = { a: i, b: i + 1, c: i + 2, d: i + 3 };
    keep.push(cell);
    if (keep.length >= 1024) {
      keep = [];
    }
  }
}

// `any`, so the cell is a real heap object the caller's frame has to root.
function makeCell(n: number): any {
  return { a: n, b: "seven" };
}

function main(): void {
  const m = methodName();

  // 1. A throw inside a 9-param dynamically dispatched method, caught here.
  let caught = "";
  try {
    make(0)[m](1, 2, 3, 4, 5, 6, 7, 8, "throw");
  } catch (e: any) {
    caught = e.message;
  }
  console.log("throw-through-method: " + caught);

  // 2. A throw inside a 9-param constructor reached through a class value.
  const K = makeClass("ctor");
  caught = "";
  try {
    new K(1, 2, 3, 4, 5, 6, 7, 8, 9);
  } catch (e: any) {
    caught = e.message;
  }
  console.log("throw-through-ctor: " + caught);

  // 3. A collection inside the callee must still see THIS frame's roots:
  //    `keep` is young, live only here, and read after the call returns.
  const keep = makeCell(7);
  const r = make(2)[m](1, 2, 3, 4, 5, 6, 7, 8, "churn");
  console.log("after-churn: " + keep.a + "/" + keep.b + " " + r);

  // CONTRACT, NOT A GAP: the same dynamic dispatch with nothing unwinding
  // through it. Here so a fix that mis-passes a stacked argument is caught.
  console.log("control: " + make(1)[m](1, 2, 3, 4, 5, 6, 7, 8, "plain"));
}

main();
