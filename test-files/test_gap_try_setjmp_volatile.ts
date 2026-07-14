// Adversarial suite for the setjmp/longjmp volatile-slot contract (#6385).
//
// Perry lowers try/catch to setjmp/longjmp. Any automatic (alloca-backed)
// local that is MODIFIED between the setjmp and the longjmp and then READ on
// the post-longjmp path must live in memory across the setjmp — otherwise
// LLVM's mem2reg/SROA promotes it to an SSA register, the register allocator
// parks it in a callee-saved register, and longjmp restores that register to
// its setjmp-time value: the try-body mutation silently evaporates.
//
// Every case below writes a local inside a `try` body (directly, in a nested
// block, in a loop, in a nested try, in a closure) and then reads it after the
// longjmp has fired. A miscompile shows up as a stale value, NOT a crash.
//
// MUST be checked at release/-O2 — perry-dev (opt-level=1) can hide it.

let out = "";
function log(label: string, v: unknown): void {
  out += label + "=" + String(v) + "\n";
}

// 1. Plain: written in try, read in catch.
function c1(): number {
  let acc = 0;
  try {
    acc = 41;
    throw new Error("x");
  } catch (e) {
    acc += 1;
  }
  return acc;
}
log("c1", c1()); // 42

// 2. Written in try, read in FINALLY.
function c2(): number {
  let acc = 0;
  try {
    acc = 7;
    throw new Error("x");
  } catch (e) {
    acc *= 2;
  } finally {
    acc += 100;
  }
  return acc;
}
log("c2", c2()); // 114

// 3. Written in try, read AFTER the whole try statement.
function c3(): number {
  let acc = 0;
  try {
    acc = 5;
    throw new Error("x");
  } catch (e) {
    // does not touch acc
  }
  return acc + 1;
}
log("c3", c3()); // 6

// 4. `var` (function-scoped) rather than `let`.
function c4(): number {
  var acc = 0;
  var i = 0;
  for (i = 0; i < 10; i++) {
    try {
      acc += 1;
      if (i === 3) throw i;
      acc += 10;
    } catch (e) {
      acc += 100;
    }
  }
  return acc;
}
log("c4", c4()); // 9 iters * 11 + 1 iter * (1 + 100) = 99 + 101 = 200

// 5. Written inside a LOOP inside the try; throw after some iterations.
function c5(): number {
  let acc = 0;
  try {
    for (let i = 0; i < 100; i++) {
      acc += i;
      if (i === 10) throw new Error("stop");
    }
  } catch (e) {
    acc += 1000;
  }
  return acc;
}
log("c5", c5()); // 0..10 = 55, + 1000 = 1055

// 6. Written in a NESTED try's body; outer catch reads it.
function c6(): number {
  let acc = 0;
  try {
    try {
      acc += 1;
      throw new Error("inner");
    } catch (e) {
      acc += 2;
    }
    acc += 4;
    throw new Error("outer");
  } catch (e) {
    acc += 8;
  }
  return acc;
}
log("c6", c6()); // 15

// 7. Written in try AND again in catch; read after.
function c7(): number {
  let acc = 1;
  try {
    acc = acc * 3; // 3
    throw new Error("x");
  } catch (e) {
    acc = acc * 5; // 15
  }
  return acc;
}
log("c7", c7()); // 15

// 8. Throw BEFORE the write — catch must see the pre-try value.
function c8(): number {
  let acc = 9;
  try {
    if (acc === 9) throw new Error("early");
    acc = 999; // never runs
  } catch (e) {
    acc += 1;
  }
  return acc;
}
log("c8", c8()); // 10

// 9. Written in a closure created inside the try, called inside the try.
function c9(): number {
  let acc = 0;
  try {
    const bump = () => {
      acc += 3;
    };
    bump();
    bump();
    throw new Error("x");
  } catch (e) {
    acc += 1;
  }
  return acc;
}
log("c9", c9()); // 7

// 10. Object / array field mutated in the try (heap — must be unaffected).
function c10(): string {
  const o: { n: number } = { n: 0 };
  const a: number[] = [0];
  try {
    o.n = 5;
    a[0] = 6;
    a.push(7);
    throw new Error("x");
  } catch (e) {
    o.n += 1;
  }
  return o.n + "," + a[0] + "," + a[1] + "," + a.length;
}
log("c10", c10()); // 6,6,7,2

// 11. Many locals, only some written in the try (checks we don't lose the
//     un-written ones either — those are live across the setjmp).
function c11(): string {
  let a = 1;
  let b = 2;
  let c = 3;
  let d = 4;
  try {
    a = 10;
    c = 30;
    throw new Error("x");
  } catch (e) {
    a += b; // 12
    c += d; // 34
  }
  return a + "," + b + "," + c + "," + d;
}
log("c11", c11()); // 12,2,34,4

// 12. try/finally with NO catch: finally reads a try-body write, then the
//     throw re-propagates to an outer catch which also reads it.
function c12(): number {
  let acc = 0;
  try {
    try {
      acc = 3;
      throw new Error("x");
    } finally {
      acc += 10; // 13
    }
  } catch (e) {
    acc += 100; // 113
  }
  return acc;
}
log("c12", c12()); // 113

// 13. Write in the CATCH body of a try that HAS a finally (the catch body is
//     itself setjmp-protected so the finally can re-run on a catch-body throw).
function c13(): number {
  let acc = 0;
  try {
    try {
      throw new Error("a");
    } catch (e) {
      acc = 1;
      throw new Error("b"); // escapes the catch → finally must still run
    } finally {
      acc += 10; // 11
    }
  } catch (e) {
    acc += 100; // 111
  }
  return acc;
}
log("c13", c13()); // 111

// 14. String local written in the try (pointer-tagged, GC-tracked).
function c14(): string {
  let s = "a";
  try {
    s = s + "b";
    throw new Error("x");
  } catch (e) {
    s = s + "c";
  }
  return s;
}
log("c14", c14()); // abc

// 15. Compound: accumulator + counter, throw on odd — the #6385 shape.
function c15(): number {
  let acc = 0;
  for (let i = 0; i < 1000; i++) {
    try {
      if ((i & 1) === 0) throw i;
      acc += 1;
    } catch (e) {
      acc += 2;
    }
  }
  return acc;
}
log("c15", c15()); // 500*2 + 500*1 = 1500

// 16. Destructured locals written inside the try.
function c16(): string {
  let x = 0;
  let y = 0;
  try {
    [x, y] = [11, 22];
    throw new Error("x");
  } catch (e) {
    x += 1;
  }
  return x + "," + y;
}
log("c16", c16()); // 12,22

// 17. Catch parameter reassigned inside the catch body, with a finally
//     (the catch body is setjmp-protected).
function c17(): string {
  let r = "";
  try {
    try {
      throw "orig";
    } catch (e) {
      e = String(e) + "-mut";
      r = String(e);
      throw new Error("again");
    } finally {
      r += "|fin";
    }
  } catch (e2) {
    r += "|outer";
  }
  return r;
}
log("c17", c17()); // orig-mut|fin|outer

// 18. Async: write in try, read in catch after an awaited rejection.
async function c18(): Promise<number> {
  let acc = 0;
  try {
    acc = 4;
    await Promise.reject(new Error("x"));
    acc = 999;
  } catch (e) {
    acc += 1;
  }
  return acc;
}

// 19. Generator: write in try, read in catch after a throw across a yield.
function* gen(): Generator<number, number, unknown> {
  let acc = 0;
  try {
    acc = 2;
    yield 1;
    acc = 999;
  } catch (e) {
    acc += 3;
  }
  return acc;
}
function c19(): number {
  const g = gen();
  g.next();
  const r = g.throw(new Error("x")) as IteratorResult<number, number>;
  return r.value as number;
}
log("c19", c19()); // 5

// 20. Deeply-nested try in a loop in a try, all sharing one accumulator.
function c20(): number {
  let acc = 0;
  try {
    for (let i = 0; i < 5; i++) {
      try {
        acc += 1;
        if (i % 2 === 0) throw i;
        acc += 10;
      } catch (e) {
        acc += 100;
      }
    }
    throw new Error("outer");
  } catch (e) {
    acc += 1000;
  }
  return acc;
}
log("c20", c20()); // i=0,2,4 -> 101 each = 303; i=1,3 -> 11 each = 22; +1000 = 1325

async function main(): Promise<void> {
  log("c18", await c18()); // 5
  process.stdout.write(out);
}
main();
